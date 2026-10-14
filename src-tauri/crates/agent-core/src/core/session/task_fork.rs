//! Task fork: a *compressed*, independent runtime child of a main session and
//! an exactly-once, asynchronous summary hand-back to that main session.
//!
//! Invariants (each has a test in `task_fork_tests.rs`):
//! * A child is published only after the summarizer returned a complete,
//!   non-empty summary. A failed compression never leaves a child row and never
//!   falls back to the full parent history.
//! * The child transcript starts with exactly one seed message (the summary);
//!   it never contains the parent's raw rows.
//! * The parent transcript is append-only. Finishing a task appends ONE
//!   source-identified user message after the current parent tail inside a
//!   single `BEGIN IMMEDIATE` transaction that also writes the receipt.
//!   Existing parent rows (and therefore system/tools/prefix bytes) are never
//!   rewritten.
//! * Duplicate finish / retry / restart cannot append twice: the append id is
//!   deterministic (`task-summary:<id>`, a primary key) and the job moves to
//!   `appended` in the same transaction.
//! * A summary failure keeps the task, records the error and stays retryable;
//!   automatic review is never a precondition.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::model_context::compaction::ContextCompactor;
use crate::core::session::journey_canonical::{validate_commit_with_connection, DurableAnchor};
use crate::core::session::journey_lifecycle::RuntimeProvenance;
use database::db::{get_connection, with_sessions_writer};

pub const SUMMARY_MESSAGE_PREFIX: &str = "【Task 摘要】";
pub const SEED_MESSAGE_PREFIX: &str = "【Task 上下文（压缩）】";
const MAX_NAME_CHARS: usize = 80;
const MAX_TRANSCRIPT_CHARS: usize = 600_000;
const TOOL_TEXT_CHARS: usize = 1_500;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS task_forks(
 id TEXT PRIMARY KEY,
 parent_session_id TEXT NOT NULL,
 intent_id TEXT NOT NULL,
 name TEXT NOT NULL,
 child_session_id TEXT UNIQUE,
 anchor_message_id TEXT NOT NULL,
 anchor_sequence INTEGER NOT NULL,
 anchor_json TEXT NOT NULL,
 parent_prefix_hash TEXT NOT NULL,
 source_message_count INTEGER NOT NULL,
 source_tokens INTEGER NOT NULL,
 source_truncated INTEGER NOT NULL DEFAULT 0,
 seed_message_id TEXT,
 seed_hash TEXT,
 seed_tokens INTEGER,
 model_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 protocol TEXT NOT NULL,
 compress_usage_json TEXT,
 state TEXT NOT NULL,
 error TEXT,
 compress_attempts INTEGER NOT NULL DEFAULT 0,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 UNIQUE(parent_session_id,intent_id)
);
CREATE TABLE IF NOT EXISTS task_fork_summary_jobs(
 task_fork_id TEXT PRIMARY KEY,
 state TEXT NOT NULL,
 outcome TEXT NOT NULL,
 frozen_child_end_sequence INTEGER NOT NULL,
 frozen_child_hash TEXT NOT NULL,
 attempts INTEGER NOT NULL DEFAULT 0,
 lease_token TEXT,
 summary_text TEXT,
 summary_hash TEXT,
 summary_model_id TEXT,
 summary_account_id TEXT,
 summary_protocol TEXT,
 usage_json TEXT,
 error TEXT,
 deferred_reason TEXT,
 parent_message_id TEXT,
 parent_sequence INTEGER,
 appended_after_message_id TEXT,
 appended_after_sequence INTEGER,
 parent_prefix_hash_before TEXT,
 parent_prefix_hash_after TEXT,
 appended_at TEXT,
 event_published_at TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 finish_seq INTEGER,
 merge_index INTEGER
);";

// ───────────────────────── model & wire types ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    Compress,
    Finish,
}

pub struct SummaryRequest<'a> {
    pub kind: SummaryKind,
    pub task_name: &'a str,
    pub transcript: &'a str,
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone, Default)]
pub struct SummaryOutput {
    pub text: String,
    /// Provider-reported usage (prompt/completion tokens, cache fields when present).
    pub usage: Vec<(String, i64)>,
}

/// One-shot summarizer bound to a resolved runtime. Never silently switches
/// model/account/protocol: `provenance()` is what actually answers.
#[async_trait::async_trait]
pub trait TaskForkSummarizer: Send + Sync {
    fn provenance(&self) -> RuntimeProvenance;
    async fn summarize(&self, request: SummaryRequest<'_>) -> Result<SummaryOutput, String>;
}

pub struct ParentLockGuard(#[allow(dead_code)] Option<tokio::sync::OwnedMutexGuard<()>>);
impl ParentLockGuard {
    pub fn held(guard: tokio::sync::OwnedMutexGuard<()>) -> Self {
        Self(Some(guard))
    }
    pub fn unregistered() -> Self {
        Self(None)
    }
}

#[async_trait::async_trait]
pub trait TaskForkHost: Send + Sync {
    /// Summarizer for finishing `link`: the child's runtime, else the parent's
    /// when it is the same provenance as the task was compressed with.
    async fn summarizer_for_finish(
        &self,
        link: &TaskForkLink,
    ) -> Result<Arc<dyn TaskForkSummarizer>, String>;
    /// Non-blocking claim of the parent's turn lock. `Err` means a turn is running.
    async fn try_lock_parent(&self, parent_session_id: &str) -> Result<ParentLockGuard, String>;
    /// Make an already-durable transcript message visible in the chat UI (event cache).
    /// Idempotent: the event id is derived from `message_id`.
    async fn publish_message_event(
        &self,
        session_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTaskForkRequest {
    pub intent_id: String,
    pub name: String,
    pub anchor: DurableAnchor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinishTaskForkRequest {
    pub task_fork_id: String,
    /// completed | partially_completed | paused | abandoned | redirected
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskForkLink {
    pub id: String,
    pub parent_session_id: String,
    pub child_session_id: Option<String>,
    pub name: String,
    pub state: String,
    pub error: Option<String>,
    pub anchor_message_id: String,
    pub anchor_sequence: i64,
    pub parent_prefix_hash: String,
    pub source_message_count: i64,
    pub source_tokens: i64,
    pub source_truncated: bool,
    pub seed_message_id: Option<String>,
    pub seed_hash: Option<String>,
    pub seed_tokens: Option<i64>,
    pub model_id: String,
    pub account_id: String,
    pub protocol: String,
    pub compress_attempts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskForkJob {
    pub state: String,
    pub outcome: String,
    pub attempts: i64,
    pub error: Option<String>,
    pub deferred_reason: Option<String>,
    pub frozen_child_end_sequence: i64,
    pub summary_chars: Option<i64>,
    pub summary_hash: Option<String>,
    pub summary_model_id: Option<String>,
    pub parent_message_id: Option<String>,
    pub parent_sequence: Option<i64>,
    pub appended_after_message_id: Option<String>,
    pub appended_after_sequence: Option<i64>,
    pub parent_prefix_hash_before: Option<String>,
    pub parent_prefix_hash_after: Option<String>,
    pub event_published_at: Option<String>,
    /// Per-parent order in which the user finished Tasks; summaries merge in this order.
    pub finish_seq: Option<i64>,
    /// 1-based position at which this summary was merged into the parent (set on append).
    pub merge_index: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskForkView {
    #[serde(flatten)]
    pub link: TaskForkLink,
    pub job: Option<TaskForkJob>,
    /// The Task child accepts no new turns (finishing/finished); `cancel_finish` reopens a finishing one.
    pub read_only: bool,
    /// True only when this call published the child (not an idempotent replay).
    pub newly_published: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DriveOptions {
    pub append_retries: u32,
    pub retry_delay: Duration,
}
impl Default for DriveOptions {
    fn default() -> Self {
        Self {
            append_retries: 20,
            retry_delay: Duration::from_millis(1500),
        }
    }
}

// ───────────────────────── small helpers ─────────────────────────

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn err<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

pub fn outcome_label(outcome: &str) -> Result<&'static str, String> {
    Ok(match outcome {
        "completed" => "完成",
        "partially_completed" => "部分完成",
        "paused" => "暂停",
        "abandoned" => "放弃",
        "redirected" => "转向",
        _ => return Err("task_fork_invalid_outcome".into()),
    })
}

fn inflight() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

struct Inflight {
    key: String,
    flag: Arc<AtomicBool>,
}
impl Inflight {
    fn acquire(key: &str) -> Option<Self> {
        let mut map = inflight().lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(key) {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        map.insert(key.to_string(), flag.clone());
        Some(Self {
            key: key.to_string(),
            flag,
        })
    }
}
impl Drop for Inflight {
    fn drop(&mut self) {
        inflight()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.key);
    }
}

fn deterministic_ids(owner: &str, intent: &str) -> (String, String) {
    let hash = digest(&["task-fork", owner, intent]);
    let id = format!("tf-{}", &hash[..32]);
    let child = format!("{owner}-task-{}", &hash[..16]);
    (id, child)
}

pub fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(err)?;
    // Databases created before merge ordering existed lack these columns.
    for column in ["finish_seq", "merge_index"] {
        let present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_fork_summary_jobs') WHERE name=?1)",
                [column],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !present {
            conn.execute(
                &format!("ALTER TABLE task_fork_summary_jobs ADD COLUMN {column} INTEGER"),
                [],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

// ───────────────────────── transcript view ─────────────────────────

#[derive(Debug, Clone)]
struct Row {
    id: String,
    sequence: i64,
    role: String,
    content: String,
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    tool_input: Option<String>,
    tool_output: Option<String>,
    created_at: String,
    compact_from_sequence: Option<i64>,
}

fn load_rows(
    conn: &Connection,
    session: &str,
    max_sequence: Option<i64>,
) -> Result<Vec<Row>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id,sequence,role,content,tool_name,tool_call_id,tool_input,tool_output,created_at,compact_from_sequence
             FROM agent_messages WHERE session_id=?1 AND sequence<=?2 ORDER BY sequence,id",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![session, max_sequence.unwrap_or(i64::MAX)], |r| {
            Ok(Row {
                id: r.get(0)?,
                sequence: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                tool_name: r.get(4)?,
                tool_call_id: r.get(5)?,
                tool_input: r.get(6)?,
                tool_output: r.get(7)?,
                created_at: r.get(8)?,
                compact_from_sequence: r.get(9)?,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    Ok(rows)
}

fn rows_hash(rows: &[Row]) -> String {
    let mut hasher = Sha256::new();
    let mut feed = |s: &str| {
        hasher.update((s.len() as u64).to_le_bytes());
        hasher.update(s.as_bytes());
    };
    for row in rows {
        feed(&row.id);
        feed(&row.sequence.to_string());
        feed(&row.role);
        feed(&row.content);
        feed(row.tool_name.as_deref().unwrap_or(""));
        feed(row.tool_call_id.as_deref().unwrap_or(""));
        feed(row.tool_input.as_deref().unwrap_or(""));
        feed(row.tool_output.as_deref().unwrap_or(""));
        feed(&row.created_at);
        feed(
            &row.compact_from_sequence
                .map(|v| v.to_string())
                .unwrap_or_default(),
        );
    }
    format!("{:x}", hasher.finalize())
}

fn max_sequence(conn: &Connection, session: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COALESCE(MAX(sequence),-1) FROM agent_messages WHERE session_id=?1",
        [session],
        |r| r.get(0),
    )
    .map_err(err)
}

/// Rows visible to the model: the latest compact summary plus the surviving tail.
fn visible_rows(rows: &[Row]) -> Vec<&Row> {
    let boundary = rows
        .iter()
        .filter(|row| row.compact_from_sequence.is_some())
        .max_by_key(|row| row.sequence);
    match boundary {
        None => rows.iter().collect(),
        Some(boundary) => {
            let from = boundary.compact_from_sequence.unwrap_or(0);
            rows.iter()
                .filter(|row| {
                    row.id == boundary.id
                        || (row.sequence >= from && row.compact_from_sequence.is_none())
                })
                .collect()
        }
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…[已截断]")
}

struct TranscriptView {
    text: String,
    message_count: usize,
    tokens: usize,
    truncated: bool,
}

fn build_transcript(rows: &[Row]) -> TranscriptView {
    let visible = visible_rows(rows);
    let mut lines: Vec<String> = Vec::with_capacity(visible.len());
    let mut raw: Vec<serde_json::Value> = Vec::with_capacity(visible.len());
    for row in &visible {
        let (label, body, raw_body) = match row.role.as_str() {
            "user" => ("用户", clip(&row.content, 20_000), row.content.clone()),
            "assistant" => ("助手", clip(&row.content, 20_000), row.content.clone()),
            "system" => (
                "此前对话摘要",
                clip(&row.content, 20_000),
                row.content.clone(),
            ),
            "tool_call" => {
                let input = row.tool_input.clone().unwrap_or_default();
                let name = row.tool_name.clone().unwrap_or_default();
                lines.push(format!(
                    "[工具调用 {name}] {}",
                    clip(&input, TOOL_TEXT_CHARS)
                ));
                raw.push(serde_json::json!({"role":"assistant","content":input}));
                continue;
            }
            "tool_result" => {
                let output = row
                    .tool_output
                    .clone()
                    .unwrap_or_else(|| row.content.clone());
                let name = row.tool_name.clone().unwrap_or_default();
                lines.push(format!(
                    "[工具结果 {name}] {}",
                    clip(&output, TOOL_TEXT_CHARS)
                ));
                raw.push(serde_json::json!({"role":"user","content":output}));
                continue;
            }
            other => (other, clip(&row.content, 2_000), row.content.clone()),
        };
        lines.push(format!("[{label}] {body}"));
        raw.push(serde_json::json!({"role": "user", "content": raw_body}));
    }
    let tokens = ContextCompactor::estimate_messages_tokens(&raw);
    let mut text = lines.join("\n\n");
    let mut truncated = false;
    if text.chars().count() > MAX_TRANSCRIPT_CHARS {
        let skip = text.chars().count() - MAX_TRANSCRIPT_CHARS;
        text = format!(
            "[较早的消息已省略以适应摘要输入上限]\n\n{}",
            text.chars().skip(skip).collect::<String>()
        );
        truncated = true;
    }
    TranscriptView {
        text,
        message_count: visible.len(),
        tokens,
        truncated,
    }
}

/// Mirrors the historical-fork rule: every tool call in the visible prefix must be
/// answered before the next conversational row.
fn prefix_tool_pairing_ok(rows: &[Row]) -> bool {
    let mut pending = std::collections::HashSet::<String>::new();
    for row in visible_rows(rows) {
        match row.role.as_str() {
            "tool_call" => {
                let Some(id) = row.tool_call_id.clone().filter(|v| !v.is_empty()) else {
                    return false;
                };
                if !pending.insert(id) {
                    return false;
                }
            }
            "tool_result" => {
                if !row
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| pending.remove(id))
                {
                    return false;
                }
            }
            "user" | "assistant" | "system" => {
                if !pending.is_empty() {
                    return false;
                }
            }
            _ => {}
        }
    }
    pending.is_empty()
}

/// Open tool calls after the last user message mean a turn is mid-flight.
fn tail_has_open_tool_call(rows: &[Row]) -> bool {
    let start = rows
        .iter()
        .rposition(|row| row.role == "user")
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut pending = std::collections::HashSet::<String>::new();
    for row in &rows[start..] {
        match row.role.as_str() {
            "tool_call" => {
                if let Some(id) = row.tool_call_id.clone() {
                    pending.insert(id);
                }
            }
            "tool_result" => {
                if let Some(id) = &row.tool_call_id {
                    pending.remove(id);
                }
            }
            _ => {}
        }
    }
    !pending.is_empty()
}

// ───────────────────────── prompts ─────────────────────────

pub fn compress_prompt(task_name: &str) -> String {
    format!(
        "你是上下文压缩器。用户要从下面这段主线对话中分叉出一个独立的 Task「{task_name}」。\
请用中文输出一份最小但充分的上下文摘要，供 Task 在**看不到原对话**的前提下继续工作。必须覆盖：\
1) 用户目标与约束；2) 已做出的关键决定与理由；3) 与 Task 相关的文件、命令、数据、接口及其当前状态；\
4) 已知错误、坑与未决问题；5) 最近一条用户消息的原意。\
不要复述寒暄和完整过程，不要编造对话中没有的信息，不要超过必要长度。直接输出摘要正文。"
    )
}

pub fn finish_prompt(task_name: &str, outcome_label: &str) -> String {
    format!(
        "你是 Task 结果归并器。下面是独立 Task「{task_name}」的完整对话，其结果状态为「{outcome_label}」。\
请用中文输出一份给**主线对话**继续使用的结论摘要：目标、实际结论或产出、关键决定、创建/修改的文件或工件、\
未决问题与建议的下一步。只写结论，不要复述过程，不要编造 Task 中没有发生的事。直接输出摘要正文。"
    )
}

// ───────────────────────── production summarizer ─────────────────────────

pub struct SideQuerySummarizer {
    provider: Arc<dyn crate::providers::traits::LLMProvider>,
    model: String,
    account_id: Option<String>,
    provenance: RuntimeProvenance,
}

impl SideQuerySummarizer {
    pub fn new(
        provider: Arc<dyn crate::providers::traits::LLMProvider>,
        model: String,
        account_id: Option<String>,
    ) -> Result<Self, String> {
        let account = account_id
            .clone()
            .or_else(|| provider.credential_source().map(str::to_string))
            .unwrap_or_default();
        let provenance = RuntimeProvenance {
            model_id: model.clone(),
            account_id: account,
            protocol: provider.provider_name().to_string(),
        };
        if provenance.model_id.trim().is_empty()
            || provenance.account_id.trim().is_empty()
            || provenance.protocol.trim().is_empty()
        {
            return Err("task_fork_runtime_provenance_missing".into());
        }
        Ok(Self {
            provider,
            model,
            account_id,
            provenance,
        })
    }
}

#[async_trait::async_trait]
impl TaskForkSummarizer for SideQuerySummarizer {
    fn provenance(&self) -> RuntimeProvenance {
        self.provenance.clone()
    }

    async fn summarize(&self, request: SummaryRequest<'_>) -> Result<SummaryOutput, String> {
        let (system, max_tokens) = match request.kind {
            SummaryKind::Compress => (compress_prompt(request.task_name), 4096u32),
            SummaryKind::Finish => (finish_prompt(request.task_name, ""), 2048u32),
        };
        let config = crate::core::side_query::SideQueryConfig {
            model: None,
            max_tokens,
            temperature: 0.0,
            system_prompt: Some(system),
            structured: None,
            account_id: self.account_id.clone(),
            // One-shot over a prefix that is never sent again: do not pay for a cache write.
            skip_cache_write: true,
        };
        let messages = vec![serde_json::json!({"role":"user","content":request.transcript})];
        let result = crate::core::side_query::side_query_typed_with_options(
            self.provider.as_ref(),
            &messages,
            &config,
            &self.model,
            request.cancel.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())?;
        if result.finish_reason == crate::providers::finish_reason::LENGTH {
            return Err(
                "task_fork_summary_truncated: 摘要被输出上限截断，拒绝使用不完整摘要".into(),
            );
        }
        let text = result.content.trim().to_string();
        if text.is_empty() {
            return Err("task_fork_summary_empty".into());
        }
        Ok(SummaryOutput {
            text,
            usage: vec![
                ("prompt_tokens".into(), result.prompt_tokens),
                ("completion_tokens".into(), result.completion_tokens),
            ],
        })
    }
}

// ───────────────────────── row mapping ─────────────────────────

fn map_link(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskForkLink> {
    Ok(TaskForkLink {
        id: r.get("id")?,
        parent_session_id: r.get("parent_session_id")?,
        child_session_id: r.get("child_session_id")?,
        name: r.get("name")?,
        state: r.get("state")?,
        error: r.get("error")?,
        anchor_message_id: r.get("anchor_message_id")?,
        anchor_sequence: r.get("anchor_sequence")?,
        parent_prefix_hash: r.get("parent_prefix_hash")?,
        source_message_count: r.get("source_message_count")?,
        source_tokens: r.get("source_tokens")?,
        source_truncated: r.get::<_, i64>("source_truncated")? != 0,
        seed_message_id: r.get("seed_message_id")?,
        seed_hash: r.get("seed_hash")?,
        seed_tokens: r.get("seed_tokens")?,
        model_id: r.get("model_id")?,
        account_id: r.get("account_id")?,
        protocol: r.get("protocol")?,
        compress_attempts: r.get("compress_attempts")?,
    })
}

fn load_link(conn: &Connection, id: &str) -> Result<Option<TaskForkLink>, String> {
    conn.query_row("SELECT * FROM task_forks WHERE id=?1", [id], map_link)
        .optional()
        .map_err(err)
}

fn load_job(conn: &Connection, id: &str) -> Result<Option<TaskForkJob>, String> {
    conn.query_row(
        "SELECT state,outcome,attempts,error,deferred_reason,frozen_child_end_sequence,
                CASE WHEN summary_text IS NULL THEN NULL ELSE length(summary_text) END,
                summary_hash,summary_model_id,parent_message_id,parent_sequence,
                appended_after_message_id,appended_after_sequence,
                parent_prefix_hash_before,parent_prefix_hash_after,event_published_at,
                finish_seq,merge_index
         FROM task_fork_summary_jobs WHERE task_fork_id=?1",
        [id],
        |r| {
            Ok(TaskForkJob {
                state: r.get(0)?,
                outcome: r.get(1)?,
                attempts: r.get(2)?,
                error: r.get(3)?,
                deferred_reason: r.get(4)?,
                frozen_child_end_sequence: r.get(5)?,
                summary_chars: r.get(6)?,
                summary_hash: r.get(7)?,
                summary_model_id: r.get(8)?,
                parent_message_id: r.get(9)?,
                parent_sequence: r.get(10)?,
                appended_after_message_id: r.get(11)?,
                appended_after_sequence: r.get(12)?,
                parent_prefix_hash_before: r.get(13)?,
                parent_prefix_hash_after: r.get(14)?,
                event_published_at: r.get(15)?,
                finish_seq: r.get(16)?,
                merge_index: r.get(17)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

fn view(conn: &Connection, id: &str, newly_published: bool) -> Result<TaskForkView, String> {
    let link = load_link(conn, id)?.ok_or("task_fork_not_found")?;
    let job = load_job(conn, id)?;
    Ok(TaskForkView {
        read_only: is_read_only_state(&link.state),
        link,
        job,
        newly_published,
    })
}

pub fn view_by_id(task_fork_id: &str) -> Result<TaskForkView, String> {
    let conn = get_connection().map_err(err)?;
    ensure_schema(&conn)?;
    view(&conn, task_fork_id, false)
}

/// Owner isolation: a caller may only see links where it is the parent or the child.
pub fn list_for_owner(owner: &str) -> Result<Vec<TaskForkView>, String> {
    let conn = get_connection().map_err(err)?;
    ensure_schema(&conn)?;
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT id FROM task_forks WHERE parent_session_id=?1 OR child_session_id=?1 ORDER BY created_at,id")
            .map_err(err)?;
        let rows = stmt
            .query_map([owner], |r| r.get(0))
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(err)?;
        rows
    };
    ids.iter().map(|id| view(&conn, id, false)).collect()
}

/// Returns the link only when `owner` is its parent or child.
pub fn authorize(
    conn: &Connection,
    owner: &str,
    task_fork_id: &str,
) -> Result<TaskForkLink, String> {
    let link = load_link(conn, task_fork_id)?.ok_or("task_fork_not_found")?;
    if link.parent_session_id != owner && link.child_session_id.as_deref() != Some(owner) {
        return Err("task_fork_owner_mismatch".into());
    }
    Ok(link)
}

// ───────────────────────── read-only lock ─────────────────────────

fn is_read_only_state(state: &str) -> bool {
    matches!(state, "finishing" | "finished")
}

/// Why `session_id` may not start new turns, when it is a Task child that is being (or has been)
/// finished. `None` for every other session. The error text is user-facing and stable-prefixed.
pub fn child_write_block(conn: &Connection, session_id: &str) -> Result<Option<String>, String> {
    if !crate::coordination::agent_org_history_store::table_exists(conn, "task_forks")
        .map_err(err)?
    {
        return Ok(None);
    }
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT name,state FROM task_forks WHERE child_session_id=?1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    Ok(row.filter(|(_, state)| is_read_only_state(state)).map(|(name, state)| {
        if state == "finished" {
            format!("task_fork_child_read_only: Task「{name}」已结束并把摘要归并到主线，该会话只读；如需继续请回主线新建 Task")
        } else {
            format!("task_fork_child_read_only: Task「{name}」正在结束并总结，该会话暂时只读；取消结束后可继续")
        }
    }))
}

// ───────────────────────── restore from persisted config ─────────────────────────

/// Summarizer built from the session's *persisted, legal* configuration (model + exactly one of
/// account / credential source + harness), for use when the session has no live runtime
/// (typically right after an app restart). It never picks a default model, never applies fallback
/// models, and fails closed with an explicit reason when the persisted configuration is absent,
/// ambiguous or no longer valid.
pub async fn summarizer_from_persisted_config(
    session_id: &str,
) -> Result<SideQuerySummarizer, String> {
    let (provider, model, account) = provider_from_persisted_config(session_id).await?;
    SideQuerySummarizer::new(provider, model, account)
}

/// The provider, exact model and account (if the credential owner is an account) that a session's
/// persisted configuration resolves to. Shared by the Task-fork summarizer and the Journey review
/// executor so neither can drift into a default model or a fallback.
pub async fn provider_from_persisted_config(
    session_id: &str,
) -> Result<(Arc<dyn crate::providers::traits::LLMProvider>, String, Option<String>), String> {
    let sid = session_id.to_string();
    let (record, workspace) = tokio::task::spawn_blocking(move || {
        let record = crate::session::persistence::get_session(&sid).map_err(err)?;
        let workspace = crate::session::persistence::load_workspace(&sid).map_err(err)?;
        Ok::<_, String>((record, workspace))
    })
    .await
    .map_err(err)??;
    let record = record.ok_or_else(|| {
        format!("task_fork_runtime_unavailable: 会话 {session_id} 没有持久记录，无法恢复模型配置")
    })?;
    let model = record
        .model
        .filter(|m| !m.trim().is_empty())
        .ok_or_else(|| {
            "task_fork_session_config_invalid: 会话没有持久化的模型，拒绝选择默认模型".to_string()
        })?;
    let account = record.account_id.filter(|a| !a.trim().is_empty());
    let source = record.credential_source.filter(|c| !c.trim().is_empty());
    if account.is_some() == source.is_some() {
        return Err(
            "task_fork_session_config_invalid: 会话必须恰好有一个凭据归属（账号或凭据源），当前为缺失或冲突"
                .into(),
        );
    }
    let native = record
        .native_harness_type
        .as_deref()
        .filter(|v| !v.is_empty())
        .map(|v| {
            core_types::providers::NativeHarnessType::parse(v).ok_or_else(|| {
                format!("task_fork_session_config_invalid: 未知的 native harness {v:?}")
            })
        })
        .transpose()?;
    let provider = crate::providers::factory::create_provider_with_selection_preflight(
        &model,
        account.as_deref(),
        source.as_deref(),
        &crate::config::ReliabilityConfig::default(),
        native,
        workspace,
    )
    .await
    .map_err(|e| {
        format!("task_fork_session_config_invalid: 无法用持久配置构建 provider（{model}）：{e}")
    })?;
    Ok((Arc::from(provider), model, account))
}

// ───────────────────────── create ─────────────────────────

pub async fn create_task_fork(
    owner: &str,
    request: CreateTaskForkRequest,
    summarizer: &dyn TaskForkSummarizer,
) -> Result<TaskForkView, String> {
    let name = request.name.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err("task_fork_invalid_name".into());
    }
    if request.intent_id.trim().is_empty() || request.intent_id.len() > 256 {
        return Err("task_fork_invalid_intent".into());
    }
    if request.anchor.owner_session_id != owner || request.anchor.source_session_id != owner {
        return Err("task_fork_anchor_owner_mismatch".into());
    }
    let (id, child_id) = deterministic_ids(owner, &request.intent_id);
    let provenance = summarizer.provenance();
    if provenance.model_id.trim().is_empty()
        || provenance.account_id.trim().is_empty()
        || provenance.protocol.trim().is_empty()
    {
        return Err("task_fork_runtime_provenance_missing".into());
    }
    let Some(guard) = Inflight::acquire(&format!("create:{id}")) else {
        return Err("task_fork_compression_in_progress".into());
    };

    // Phase A: validate + reserve (no LLM, short writer).
    let phase_a = {
        let owner = owner.to_string();
        let request = request.clone();
        let (id, name, prov) = (id.clone(), name.clone(), provenance.clone());
        tokio::task::spawn_blocking(move || reserve(&owner, &request, &id, &name, &prov))
            .await
            .map_err(err)??
    };
    let rows = match phase_a {
        Reserved::AlreadyPublished => {
            let conn = get_connection().map_err(err)?;
            return view(&conn, &id, false);
        }
        Reserved::Compress { rows } => rows,
    };

    // Phase B: compression (no locks held; the parent can keep running).
    let transcript = build_transcript(&rows);
    let outcome = summarizer
        .summarize(SummaryRequest {
            kind: SummaryKind::Compress,
            task_name: &name,
            transcript: &transcript.text,
            cancel: Some(guard.flag.clone()),
        })
        .await;
    let output = match outcome {
        Ok(output) if !output.text.trim().is_empty() => output,
        Ok(_) => {
            record_compress_failure(&id, "task_fork_summary_empty");
            return Err("task_fork_summary_empty".into());
        }
        Err(e) => {
            record_compress_failure(&id, &e);
            return Err(e);
        }
    };

    // Phase C: publish child + seed + link atomically.
    let published = {
        let owner = owner.to_string();
        let (id, child_id, name) = (id.clone(), child_id.clone(), name.clone());
        let usage = serde_json::to_string(&output.usage).unwrap_or_default();
        tokio::task::spawn_blocking(move || {
            publish(&owner, &id, &child_id, &name, &output.text, &usage)
        })
        .await
        .map_err(err)?
    };
    match published {
        Ok(()) => {
            let conn = get_connection().map_err(err)?;
            view(&conn, &id, true)
        }
        Err(e) => {
            record_compress_failure(&id, &e);
            Err(e)
        }
    }
}

enum Reserved {
    AlreadyPublished,
    Compress { rows: Vec<Row> },
}

fn reserve(
    owner: &str,
    request: &CreateTaskForkRequest,
    id: &str,
    name: &str,
    provenance: &RuntimeProvenance,
) -> Result<Reserved, String> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        ensure_schema(&tx)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE session_id=?1)",
                [owner],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !exists {
            return Err("task_fork_owner_missing".into());
        }
        let anchor_json = serde_json::to_string(&request.anchor).map_err(err)?;
        if let Some(existing) = load_link(&tx, id)? {
            let stored: String = tx
                .query_row(
                    "SELECT anchor_json FROM task_forks WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if stored != anchor_json || existing.name != name {
                return Err("task_fork_intent_conflict".into());
            }
            if existing.state != "compressing" && existing.state != "compress_failed" {
                return Ok(Reserved::AlreadyPublished);
            }
            // Retry of a failed/stale compression: the frozen prefix must be unchanged.
            let rows = load_rows(&tx, owner, Some(existing.anchor_sequence))?;
            if rows_hash(&rows) != existing.parent_prefix_hash {
                return Err("task_fork_parent_prefix_changed".into());
            }
            tx.execute(
                "UPDATE task_forks SET state='compressing',error=NULL,compress_attempts=compress_attempts+1,updated_at=?2 WHERE id=?1",
                params![id, now()],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
            return Ok(Reserved::Compress { rows });
        }
        let revision = max_sequence(&tx, owner)?;
        if revision < 0 {
            return Err("task_fork_empty_parent".into());
        }
        validate_commit_with_connection(&tx, owner, &request.anchor, revision as u64, true)?;
        let anchor_sequence: i64 = tx
            .query_row(
                "SELECT sequence FROM agent_messages WHERE session_id=?1 AND id=?2",
                params![owner, request.anchor.durable_message_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        let unique: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND sequence=?2",
                params![owner, anchor_sequence],
                |r| r.get(0),
            )
            .map_err(err)?;
        if unique != 1 {
            return Err("task_fork_ambiguous_endpoint".into());
        }
        let rows = load_rows(&tx, owner, Some(anchor_sequence))?;
        if !prefix_tool_pairing_ok(&rows) {
            return Err("task_fork_unpaired_tool_prefix".into());
        }
        let transcript = build_transcript(&rows);
        let stamp = now();
        tx.execute(
            "INSERT INTO task_forks(id,parent_session_id,intent_id,name,anchor_message_id,anchor_sequence,anchor_json,
               parent_prefix_hash,source_message_count,source_tokens,source_truncated,model_id,account_id,protocol,
               state,compress_attempts,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'compressing',1,?15,?15)",
            params![
                id,
                owner,
                request.intent_id,
                name,
                request.anchor.durable_message_id,
                anchor_sequence,
                anchor_json,
                rows_hash(&rows),
                transcript.message_count as i64,
                transcript.tokens as i64,
                transcript.truncated as i64,
                provenance.model_id,
                provenance.account_id,
                provenance.protocol,
                stamp,
            ],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Reserved::Compress { rows })
    })
}

fn record_compress_failure(id: &str, reason: &str) {
    let reason = clip(reason, 500);
    let _ = with_sessions_writer(|| -> Result<(), String> {
        let conn = get_connection().map_err(err)?;
        conn.execute(
            "UPDATE task_forks SET state='compress_failed',error=?2,updated_at=?3 WHERE id=?1 AND state='compressing'",
            params![id, reason, now()],
        )
        .map_err(err)?;
        Ok(())
    });
}

fn seed_text(link: &TaskForkLink, summary: &str) -> String {
    format!(
        "{SEED_MESSAGE_PREFIX}\n来源：主线会话 {}，截至消息 {}（序号 {}）\nTask：{}（{}）\n压缩模型：{}（{}）\n以下是从主线压缩出的上下文，不是原始对话；原始对话不在本会话中。\n—— 压缩摘要 ——\n{}",
        link.parent_session_id,
        link.anchor_message_id,
        link.anchor_sequence,
        link.name,
        link.id,
        link.model_id,
        link.protocol,
        summary.trim()
    )
}

const CHILD_COPY_COLUMNS: &[&str] = &[
    "model",
    "account_id",
    "credential_source",
    "workspace_path",
    "session_type",
    "agent_definition_id",
    "key_source",
    "agent_exec_mode",
    "product_mode",
    "workspace_additional_json",
    "project_id",
    "project_name",
    "org_id",
    "native_harness_type",
];

fn publish(
    owner: &str,
    id: &str,
    child_id: &str,
    name: &str,
    summary: &str,
    usage_json: &str,
) -> Result<(), String> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        let link = load_link(&tx, id)?.ok_or("task_fork_not_found")?;
        if link.state != "compressing" {
            return Err(format!("task_fork_unexpected_state:{}", link.state));
        }
        // The frozen anchor prefix must still be byte-identical.
        let rows = load_rows(&tx, owner, Some(link.anchor_sequence))?;
        if rows_hash(&rows) != link.parent_prefix_hash {
            return Err("task_fork_parent_prefix_changed".into());
        }
        let seed = seed_text(&link, summary);
        let seed_id = format!("task-seed:{id}");
        let seed_hash = digest(&[&seed]);
        let seed_tokens = ContextCompactor::estimate_messages_tokens(&[
            serde_json::json!({"role":"user","content":seed}),
        ]);
        let stamp = now();
        let existing_cols: std::collections::HashSet<String> = {
            let mut stmt = tx
                .prepare("SELECT name FROM pragma_table_info('agent_sessions')")
                .map_err(err)?;
            let cols = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            cols
        };
        let copy: Vec<&&str> = CHILD_COPY_COLUMNS
            .iter()
            .filter(|c| existing_cols.contains(**c))
            .collect();
        let column_list = copy.iter().map(|c| format!(",{c}")).collect::<String>();
        let select_list = copy.iter().map(|c| format!(",{c}")).collect::<String>();
        tx.execute(
            &format!(
                "INSERT INTO agent_sessions(session_id,name,status,created_at,updated_at,parent_session_id{column_list})
                 SELECT ?1,?3,'idle',?4,?4,session_id{select_list} FROM agent_sessions WHERE session_id=?2"
            ),
            params![child_id, owner, format!("Task: {name}"), stamp],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at,tool_is_error)
             VALUES(?1,?2,'user',?3,0,?4,0)",
            params![seed_id, child_id, seed, stamp],
        )
        .map_err(err)?;
        tx.execute(
            "UPDATE task_forks SET child_session_id=?2,seed_message_id=?3,seed_hash=?4,seed_tokens=?5,
               compress_usage_json=?6,state='active',error=NULL,updated_at=?7 WHERE id=?1",
            params![id, child_id, seed_id, seed_hash, seed_tokens as i64, usage_json, stamp],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    })
}

// ───────────────────────── finish ─────────────────────────

/// Next merge-order number for `parent`: strictly increasing in the order the user finished Tasks.
/// Must be called inside the `BEGIN IMMEDIATE` transaction that writes the job.
fn next_finish_seq(tx: &Transaction<'_>, parent: &str) -> Result<i64, String> {
    tx.query_row(
        "SELECT COALESCE(MAX(j.finish_seq),0)+1 FROM task_fork_summary_jobs j
         JOIN task_forks f ON f.id=j.task_fork_id WHERE f.parent_session_id=?1",
        [parent],
        |r| r.get(0),
    )
    .map_err(err)
}

/// Freeze the child, create (or revive) the single summary job. Idempotent.
/// Returns the view and whether a driver should be started.
pub fn begin_finish(
    owner: &str,
    request: &FinishTaskForkRequest,
) -> Result<(TaskForkView, bool), String> {
    outcome_label(&request.outcome)?;
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        ensure_schema(&tx)?;
        let link = authorize(&tx, owner, &request.task_fork_id)?;
        let child = link
            .child_session_id
            .clone()
            .ok_or("task_fork_not_active")?;
        let job = load_job(&tx, &link.id)?;
        let stamp = now();
        let start_driver = match (link.state.as_str(), job.as_ref().map(|j| j.state.as_str())) {
            ("finished", _) | (_, Some("appended")) => false,
            ("active", None) | ("finishing", None) => {
                let rows = load_rows(&tx, &child, None)?;
                let end = rows.last().map(|r| r.sequence).unwrap_or(-1);
                let seq = next_finish_seq(&tx, &link.parent_session_id)?;
                tx.execute(
                    "INSERT INTO task_fork_summary_jobs(task_fork_id,state,outcome,frozen_child_end_sequence,frozen_child_hash,created_at,updated_at,finish_seq)
                     VALUES(?1,'queued',?2,?3,?4,?5,?5,?6)",
                    params![link.id, request.outcome, end, rows_hash(&rows), stamp, seq],
                )
                .map_err(err)?;
                tx.execute(
                    "UPDATE task_forks SET state='finishing',error=NULL,updated_at=?2 WHERE id=?1",
                    params![link.id, stamp],
                )
                .map_err(err)?;
                true
            }
            ("active", Some(_))
            | ("finishing", Some("failed"))
            | ("finishing", Some("cancelled")) => {
                // Explicit retry/re-finish: re-freeze the child, discard any unappended summary.
                let rows = load_rows(&tx, &child, None)?;
                let end = rows.last().map(|r| r.sequence).unwrap_or(-1);
                // A re-finish is a new explicit finish: it takes a fresh place in the merge order.
                let seq = next_finish_seq(&tx, &link.parent_session_id)?;
                tx.execute(
                    "UPDATE task_fork_summary_jobs SET state='queued',outcome=?2,frozen_child_end_sequence=?3,frozen_child_hash=?4,
                       summary_text=NULL,summary_hash=NULL,error=NULL,deferred_reason=NULL,lease_token=NULL,updated_at=?5,finish_seq=?6
                     WHERE task_fork_id=?1 AND state<>'appended'",
                    params![link.id, request.outcome, end, rows_hash(&rows), stamp, seq],
                )
                .map_err(err)?;
                tx.execute(
                    "UPDATE task_forks SET state='finishing',error=NULL,updated_at=?2 WHERE id=?1",
                    params![link.id, stamp],
                )
                .map_err(err)?;
                true
            }
            ("finishing", Some("queued"))
            | ("finishing", Some("running"))
            | ("finishing", Some("summarized")) => true,
            ("compressing", _) | ("compress_failed", _) => {
                return Err("task_fork_not_active".into())
            }
            _ => false,
        };
        tx.commit().map_err(err)?;
        let conn = get_connection().map_err(err)?;
        Ok((view(&conn, &link.id, false)?, start_driver))
    })
}

/// Cancel a not-yet-appended finish: the task stays active and can be finished again.
pub fn cancel_finish(owner: &str, task_fork_id: &str) -> Result<TaskForkView, String> {
    // Signal an in-flight summarizer first (best effort; the durable CAS below is authoritative).
    if let Some(flag) = inflight()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&format!("finish:{task_fork_id}"))
    {
        flag.store(true, Ordering::SeqCst);
    }
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        ensure_schema(&tx)?;
        let link = authorize(&tx, owner, task_fork_id)?;
        let job = load_job(&tx, &link.id)?.ok_or("task_fork_no_finish_in_progress")?;
        if job.state == "appended" {
            return Err("task_fork_already_appended".into());
        }
        let stamp = now();
        tx.execute(
            "UPDATE task_fork_summary_jobs SET state='cancelled',lease_token=NULL,updated_at=?2 WHERE task_fork_id=?1 AND state<>'appended'",
            params![link.id, stamp],
        )
        .map_err(err)?;
        tx.execute(
            "UPDATE task_forks SET state='active',updated_at=?2 WHERE id=?1",
            params![link.id, stamp],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        let conn = get_connection().map_err(err)?;
        view(&conn, &link.id, false)
    })
}

fn set_job_failed(id: &str, token: &str, reason: &str) {
    let reason = clip(reason, 500);
    let _ = with_sessions_writer(|| -> Result<(), String> {
        let conn = get_connection().map_err(err)?;
        conn.execute(
            "UPDATE task_fork_summary_jobs SET state='failed',error=?3,lease_token=NULL,updated_at=?4
             WHERE task_fork_id=?1 AND state='running' AND lease_token=?2",
            params![id, token, reason, now()],
        )
        .map_err(err)?;
        Ok(())
    });
}

fn requeue_job(id: &str, token: &str, reason: &str) {
    let reason = clip(reason, 300);
    let _ = with_sessions_writer(|| -> Result<(), String> {
        let conn = get_connection().map_err(err)?;
        conn.execute(
            "UPDATE task_fork_summary_jobs SET state='queued',deferred_reason=?3,lease_token=NULL,attempts=MAX(attempts-1,0),updated_at=?4
             WHERE task_fork_id=?1 AND state='running' AND lease_token=?2",
            params![id, token, reason, now()],
        )
        .map_err(err)?;
        Ok(())
    });
}

fn claim_job(id: &str) -> Result<Option<(String, TaskForkJob, String)>, String> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        let Some(job) = load_job(&tx, id)? else {
            return Ok(None);
        };
        if !matches!(job.state.as_str(), "queued" | "running") {
            return Ok(None);
        }
        // `running` with no in-process driver is a crashed attempt: take it over.
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "UPDATE task_fork_summary_jobs SET state='running',lease_token=?2,attempts=attempts+1,error=NULL,updated_at=?3 WHERE task_fork_id=?1",
            params![id, token, now()],
        )
        .map_err(err)?;
        let outcome = job.outcome.clone();
        tx.commit().map_err(err)?;
        Ok(Some((token, job, outcome)))
    })
}

/// Drive one finish job to `appended` (or to an explicit failed/cancelled/deferred
/// state). Safe to call repeatedly and concurrently: only one driver runs per job
/// in-process and every durable step is a compare-and-set.
pub async fn drive_finish_job(
    host: &dyn TaskForkHost,
    task_fork_id: &str,
    options: DriveOptions,
) -> Result<TaskForkView, String> {
    let Some(guard) = Inflight::acquire(&format!("finish:{task_fork_id}")) else {
        return view_by_id(task_fork_id);
    };
    let state = {
        let conn = get_connection().map_err(err)?;
        ensure_schema(&conn)?;
        load_job(&conn, task_fork_id)?.map(|j| j.state)
    };
    match state.as_deref() {
        Some("appended") => {
            ensure_event_published(host, task_fork_id).await;
            return view_by_id(task_fork_id);
        }
        None | Some("failed") | Some("cancelled") => return view_by_id(task_fork_id),
        _ => {}
    }

    if matches!(state.as_deref(), Some("queued") | Some("running")) {
        let id = task_fork_id.to_string();
        let claimed = tokio::task::spawn_blocking(move || claim_job(&id))
            .await
            .map_err(err)??;
        if let Some((token, job, outcome)) = claimed {
            let link = {
                let conn = get_connection().map_err(err)?;
                load_link(&conn, task_fork_id)?.ok_or("task_fork_not_found")?
            };
            let child = link
                .child_session_id
                .clone()
                .ok_or("task_fork_not_active")?;
            let summarizer = match host.summarizer_for_finish(&link).await {
                Ok(s) => s,
                Err(e) if e.starts_with("task_fork_runtime_unavailable") => {
                    // Not a failure: no live runtime yet (e.g. right after restart). Stay queued; the
                    // recovery sweep retries once a session with this provenance is active.
                    requeue_job(task_fork_id, &token, &e);
                    return view_by_id(task_fork_id);
                }
                Err(e) => {
                    set_job_failed(task_fork_id, &token, &e);
                    return view_by_id(task_fork_id);
                }
            };
            let (rows, frozen_hash) = {
                let conn = get_connection().map_err(err)?;
                let rows = load_rows(&conn, &child, Some(job.frozen_child_end_sequence))?;
                let frozen_hash: String = conn
                    .query_row("SELECT frozen_child_hash FROM task_fork_summary_jobs WHERE task_fork_id=?1", [task_fork_id], |r| r.get(0))
                    .map_err(err)?;
                (rows, frozen_hash)
            };
            if rows_hash(&rows) != frozen_hash {
                set_job_failed(task_fork_id, &token, "task_fork_child_changed_after_freeze");
                return view_by_id(task_fork_id);
            }
            let label = outcome_label(&outcome)?;
            let transcript = build_transcript(&rows);
            let prompt_name = format!("{}（结果：{}）", link.name, label);
            let result = summarizer
                .summarize(SummaryRequest {
                    kind: SummaryKind::Finish,
                    task_name: &prompt_name,
                    transcript: &transcript.text,
                    cancel: Some(guard.flag.clone()),
                })
                .await;
            let provenance = summarizer.provenance();
            match result {
                Err(e) => {
                    set_job_failed(task_fork_id, &token, &e);
                    return view_by_id(task_fork_id);
                }
                Ok(output) if output.text.trim().is_empty() => {
                    set_job_failed(task_fork_id, &token, "task_fork_summary_empty");
                    return view_by_id(task_fork_id);
                }
                Ok(output) => {
                    let id = task_fork_id.to_string();
                    let usage = serde_json::to_string(&output.usage).unwrap_or_default();
                    let hash = digest(&[&output.text]);
                    let text = output.text.trim().to_string();
                    tokio::task::spawn_blocking(move || {
                        with_sessions_writer(|| -> Result<(), String> {
                            let conn = get_connection().map_err(err)?;
                            // CAS: a cancel or newer attempt changed the lease; drop this result.
                            conn.execute(
                                "UPDATE task_fork_summary_jobs SET state='summarized',summary_text=?3,summary_hash=?4,
                                   summary_model_id=?5,summary_account_id=?6,summary_protocol=?7,usage_json=?8,lease_token=NULL,updated_at=?9
                                 WHERE task_fork_id=?1 AND state='running' AND lease_token=?2",
                                params![id, token, text, hash, provenance.model_id, provenance.account_id, provenance.protocol, usage, now()],
                            )
                            .map_err(err)?;
                            Ok(())
                        })
                    })
                    .await
                    .map_err(err)??;
                }
            }
        }
    }

    // Append stage (no provider needed — also what restart recovery runs).
    let mut attempt = 0u32;
    loop {
        let id = task_fork_id.to_string();
        let lock = host
            .try_lock_parent(&{
                let conn = get_connection().map_err(err)?;
                load_link(&conn, task_fork_id)?
                    .ok_or("task_fork_not_found")?
                    .parent_session_id
            })
            .await;
        let result = match lock {
            Ok(_guard) => tokio::task::spawn_blocking(move || append_summary_to_parent(&id))
                .await
                .map_err(err)?,
            Err(reason) => Err(AppendError::Deferred(reason)),
        };
        match result {
            Ok(_) => {
                ensure_event_published(host, task_fork_id).await;
                return view_by_id(task_fork_id);
            }
            Err(AppendError::NotReady) => return view_by_id(task_fork_id),
            Err(AppendError::Fatal(reason)) => {
                record_deferred(task_fork_id, &reason);
                return view_by_id(task_fork_id);
            }
            Err(AppendError::Deferred(reason)) => {
                record_deferred(task_fork_id, &reason);
                if attempt >= options.append_retries {
                    return view_by_id(task_fork_id);
                }
                attempt += 1;
                tokio::time::sleep(options.retry_delay).await;
            }
        }
    }
}

/// Post-commit UI publication. The transcript row is already durable; this only makes it
/// visible in the chat event cache (idempotent) and records that it happened.
async fn ensure_event_published(host: &dyn TaskForkHost, task_fork_id: &str) {
    let payload = (|| -> Result<Option<(String, String, String)>, String> {
        let conn = get_connection().map_err(err)?;
        let Some(link) = load_link(&conn, task_fork_id)? else {
            return Ok(None);
        };
        let row: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT parent_message_id,event_published_at FROM task_fork_summary_jobs WHERE task_fork_id=?1 AND state='appended'",
                [task_fork_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((Some(message_id), None)) = row else {
            return Ok(None);
        };
        let content: String = conn
            .query_row(
                "SELECT content FROM agent_messages WHERE id=?1 AND session_id=?2",
                params![message_id, link.parent_session_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(Some((link.parent_session_id, message_id, content)))
    })();
    let Ok(Some((parent, message_id, content))) = payload else {
        return;
    };
    if host
        .publish_message_event(&parent, &message_id, &content)
        .await
        .is_ok()
    {
        let id = task_fork_id.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            with_sessions_writer(|| -> Result<(), String> {
                let conn = get_connection().map_err(err)?;
                conn.execute(
                    "UPDATE task_fork_summary_jobs SET event_published_at=?2 WHERE task_fork_id=?1 AND event_published_at IS NULL",
                    params![id, now()],
                )
                .map_err(err)?;
                Ok(())
            })
        })
        .await;
    }
}

/// `(child session, seed message id, seed content)` for publishing the Task's seed to the UI.
pub fn seed_event_payload(view: &TaskForkView) -> Option<(String, String, String)> {
    let child = view.link.child_session_id.clone()?;
    let seed_id = view.link.seed_message_id.clone()?;
    let conn = get_connection().ok()?;
    let content: String = conn
        .query_row(
            "SELECT content FROM agent_messages WHERE id=?1 AND session_id=?2",
            params![seed_id, child],
            |r| r.get(0),
        )
        .ok()?;
    Some((child, seed_id, content))
}

fn record_deferred(id: &str, reason: &str) {
    let reason = clip(reason, 300);
    let _ = with_sessions_writer(|| -> Result<(), String> {
        let conn = get_connection().map_err(err)?;
        conn.execute(
            "UPDATE task_fork_summary_jobs SET deferred_reason=?2,updated_at=?3 WHERE task_fork_id=?1 AND state='summarized'",
            params![id, reason, now()],
        )
        .map_err(err)?;
        Ok(())
    });
}

#[derive(Debug)]
pub enum AppendError {
    /// Job is not (or no longer) in `summarized`.
    NotReady,
    /// Retryable: parent busy / open tool call.
    Deferred(String),
    /// Not retryable by waiting (e.g. the frozen parent prefix was rewritten).
    Fatal(String),
}

fn summary_message(
    link: &TaskForkLink,
    merge_order: (i64, i64),
    job_outcome: &str,
    summary: &str,
    summary_model: &str,
    summary_protocol: &str,
    after: &Row,
) -> Result<String, String> {
    let label = outcome_label(job_outcome)?;
    Ok(format!(
        "{SUMMARY_MESSAGE_PREFIX}来源：Task「{}」（{}）\n子会话：{}\n起点：主线消息 {}（序号 {}）\n结果：{}\n摘要模型：{}（{}）\n归并顺序：第 {} 个归并（按结束 Task 的先后，结束序 {}）\n追加位置：主线消息 {}（序号 {}）之后\n—— 以下为 Task 结论摘要，不是 Task 的完整对话 ——\n{}",
        link.name,
        link.id,
        link.child_session_id.clone().unwrap_or_default(),
        link.anchor_message_id,
        link.anchor_sequence,
        label,
        summary_model,
        summary_protocol,
        merge_order.0,
        merge_order.1,
        after.id,
        after.sequence,
        summary.trim()
    ))
}

/// The one and only parent mutation: append a single summary message after the
/// current tail and write the receipt in the same transaction.
pub fn append_summary_to_parent(task_fork_id: &str) -> Result<(), AppendError> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(|e| AppendError::Fatal(err(e)))?;
        let tx: Transaction<'_> = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| AppendError::Fatal(err(e)))?;
        let fatal = |e: String| AppendError::Fatal(e);
        let link = load_link(&tx, task_fork_id)
            .map_err(fatal)?
            .ok_or_else(|| AppendError::Fatal("task_fork_not_found".into()))?;
        let (state, outcome, summary, model, protocol): (String, String, Option<String>, Option<String>, Option<String>) = tx
            .query_row(
                "SELECT state,outcome,summary_text,summary_model_id,summary_protocol FROM task_fork_summary_jobs WHERE task_fork_id=?1",
                [task_fork_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(|e| AppendError::Fatal(err(e)))?;
        if state != "summarized" {
            return Err(AppendError::NotReady);
        }
        let summary =
            summary.ok_or_else(|| AppendError::Fatal("task_fork_summary_missing".into()))?;
        let parent = link.parent_session_id.clone();
        let rows = load_rows(&tx, &parent, None).map_err(fatal)?;
        // Frozen anchor prefix must be untouched; otherwise the lineage is orphaned.
        let anchor_rows: Vec<Row> = rows
            .iter()
            .filter(|r| r.sequence <= link.anchor_sequence)
            .cloned()
            .collect();
        if rows_hash(&anchor_rows) != link.parent_prefix_hash {
            return Err(AppendError::Fatal(
                "task_fork_parent_prefix_changed: 主线已回退或被改写，拒绝归并".into(),
            ));
        }
        let Some(tail) = rows.last().cloned() else {
            return Err(AppendError::Fatal("task_fork_parent_empty".into()));
        };
        if tail_has_open_tool_call(&rows) {
            return Err(AppendError::Deferred("parent_tool_call_open".into()));
        }
        // Merge order: summaries enter the parent in the order Tasks were finished. A Task whose
        // earlier-finished sibling is still pending (queued/running/summarized) waits for it;
        // failed/cancelled siblings need an explicit user action and never block.
        let (finish_seq, own_name): (Option<i64>, String) = (
            tx.query_row(
                "SELECT finish_seq FROM task_fork_summary_jobs WHERE task_fork_id=?1",
                [task_fork_id],
                |r| r.get(0),
            )
            .map_err(|e| AppendError::Fatal(err(e)))?,
            link.name.clone(),
        );
        let finish_seq = finish_seq.unwrap_or(0);
        let blocker: Option<(String, i64)> = tx
            .query_row(
                "SELECT f.name,COALESCE(j.finish_seq,0) FROM task_fork_summary_jobs j
                 JOIN task_forks f ON f.id=j.task_fork_id
                 WHERE f.parent_session_id=?1 AND j.task_fork_id<>?2
                   AND j.state IN ('queued','running','summarized')
                   AND (COALESCE(j.finish_seq,0)<?3 OR (COALESCE(j.finish_seq,0)=?3 AND j.task_fork_id<?2))
                 ORDER BY COALESCE(j.finish_seq,0),j.task_fork_id LIMIT 1",
                params![parent, task_fork_id, finish_seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| AppendError::Fatal(err(e)))?;
        if let Some((blocking_name, blocking_seq)) = blocker {
            return Err(AppendError::Deferred(format!(
                "merge_order_wait: 「{own_name}」（结束序 {finish_seq}）等待更早结束的 Task「{blocking_name}」（结束序 {blocking_seq}）先归并"
            )));
        }
        let merge_index: i64 = tx
            .query_row(
                "SELECT COUNT(*)+1 FROM task_fork_summary_jobs j JOIN task_forks f ON f.id=j.task_fork_id
                 WHERE f.parent_session_id=?1 AND j.state='appended'",
                [&parent],
                |r| r.get(0),
            )
            .map_err(|e| AppendError::Fatal(err(e)))?;
        let before = rows_hash(&rows);
        let message_id = format!("task-summary:{task_fork_id}");
        // Defensive receipt recovery: the id is a primary key, so a second insert cannot succeed.
        if rows.iter().any(|r| r.id == message_id) {
            return Err(AppendError::Fatal(
                "task_fork_summary_already_present_without_receipt".into(),
            ));
        }
        let content = summary_message(
            &link,
            (merge_index, finish_seq),
            &outcome,
            &summary,
            model.as_deref().unwrap_or(""),
            protocol.as_deref().unwrap_or(""),
            &tail,
        )
        .map_err(fatal)?;
        let sequence = tail.sequence + 1;
        let stamp = now();
        tx.execute(
            "INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at,tool_is_error) VALUES(?1,?2,'user',?3,?4,?5,0)",
            params![message_id, parent, content, sequence, stamp],
        )
        .map_err(|e| AppendError::Fatal(err(e)))?;
        tx.execute(
            "UPDATE agent_sessions SET updated_at=?2 WHERE session_id=?1",
            params![parent, stamp],
        )
        .map_err(|e| AppendError::Fatal(err(e)))?;
        // Journey-owned parents filter provider history by branch membership: the summary must
        // be a member of the parent's active branch like any persisted row. The role passed is
        // "system" on purpose — a synthetic hand-back is not a user-authored message and must
        // not activate a pending "next user message" Journey task.
        crate::core::session::journey_runtime_persistence::assign_inserted_message(
            &tx,
            &parent,
            &message_id,
            sequence,
            "system",
        )
        .map_err(|e| AppendError::Deferred(format!("journey_membership_unavailable: {e}")))?;
        // Old rows must hash identically after the append.
        let after_rows = load_rows(&tx, &parent, Some(tail.sequence)).map_err(fatal)?;
        let after = rows_hash(&after_rows);
        if after != before {
            return Err(AppendError::Fatal(
                "task_fork_parent_prefix_mutated_by_append".into(),
            ));
        }
        tx.execute(
            "UPDATE task_fork_summary_jobs SET state='appended',deferred_reason=NULL,parent_message_id=?2,parent_sequence=?3,
               appended_after_message_id=?4,appended_after_sequence=?5,parent_prefix_hash_before=?6,parent_prefix_hash_after=?7,
               appended_at=?8,updated_at=?8,merge_index=?9 WHERE task_fork_id=?1 AND state='summarized'",
            params![task_fork_id, message_id, sequence, tail.id, tail.sequence, before, after, stamp, merge_index],
        )
        .map_err(|e| AppendError::Fatal(err(e)))?;
        tx.execute(
            "UPDATE task_forks SET state='finished',error=NULL,updated_at=?2 WHERE id=?1",
            params![task_fork_id, stamp],
        )
        .map_err(|e| AppendError::Fatal(err(e)))?;
        tx.commit().map_err(|e| AppendError::Fatal(err(e)))
    })
}

// ───────────────────────── recovery ─────────────────────────

/// Startup / periodic recovery: crashed `running` jobs are re-queued by the next
/// driver; `summarized` jobs only need the append stage. Returns drivers run.
pub async fn recover_pending(host: &dyn TaskForkHost, options: DriveOptions) -> usize {
    let ids: Vec<String> = match tokio::task::spawn_blocking(|| -> Result<Vec<String>, String> {
        let conn = get_connection().map_err(err)?;
        ensure_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT task_fork_id FROM task_fork_summary_jobs WHERE state IN ('queued','running','summarized') OR (state='appended' AND event_published_at IS NULL) ORDER BY COALESCE(finish_seq,0),created_at,task_fork_id")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(err)?;
        Ok(rows)
    })
    .await
    {
        Ok(Ok(ids)) => ids,
        _ => return 0,
    };
    let mut ran = 0;
    for id in ids {
        if drive_finish_job(host, &id, options).await.is_ok() {
            ran += 1;
        }
    }
    ran
}

#[cfg(test)]
#[path = "task_fork_tests.rs"]
mod tests;
