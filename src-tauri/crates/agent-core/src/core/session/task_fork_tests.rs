use super::*;
use crate::core::session::journey_canonical::resolve_with_connection;
use std::sync::atomic::AtomicUsize;
use test_helpers::test_env;

const PARENT: &str = "sde-parent-1";

fn prov() -> RuntimeProvenance {
    RuntimeProvenance {
        model_id: "fake/model-1".into(),
        account_id: "acct-1".into(),
        protocol: "fake-protocol".into(),
    }
}

#[derive(Clone)]
enum Mode {
    Ok(String),
    Err(String),
    Gate,
}

struct Fake {
    mode: Mutex<Mode>,
    calls: AtomicUsize,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    last_transcript: Mutex<String>,
    last_kind: Mutex<Option<SummaryKind>>,
}
impl Fake {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            calls: AtomicUsize::new(0),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            last_transcript: Mutex::new(String::new()),
            last_kind: Mutex::new(None),
        })
    }
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
#[async_trait::async_trait]
impl TaskForkSummarizer for Fake {
    fn provenance(&self) -> RuntimeProvenance {
        prov()
    }
    async fn summarize(&self, request: SummaryRequest<'_>) -> Result<SummaryOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_transcript.lock().unwrap() = request.transcript.to_string();
        *self.last_kind.lock().unwrap() = Some(request.kind);
        let mode = self.mode.lock().unwrap().clone();
        match mode {
            Mode::Ok(text) => Ok(SummaryOutput {
                text,
                usage: vec![("prompt_tokens".into(), 10)],
            }),
            Mode::Err(e) => Err(e),
            Mode::Gate => {
                self.entered.notify_one();
                self.release.notified().await;
                Ok(SummaryOutput {
                    text: "gated summary".into(),
                    usage: vec![],
                })
            }
        }
    }
}

struct Host {
    summarizer: Arc<Fake>,
    parent_lock: Arc<tokio::sync::Mutex<()>>,
    published: Mutex<Vec<(String, String)>>,
    fail_publish: AtomicBool,
}
#[async_trait::async_trait]
impl TaskForkHost for Host {
    async fn summarizer_for_finish(
        &self,
        _link: &TaskForkLink,
    ) -> Result<Arc<dyn TaskForkSummarizer>, String> {
        Ok(self.summarizer.clone())
    }
    async fn try_lock_parent(&self, _p: &str) -> Result<ParentLockGuard, String> {
        self.parent_lock
            .clone()
            .try_lock_owned()
            .map(ParentLockGuard::held)
            .map_err(|_| "parent_busy".to_string())
    }
    async fn publish_message_event(
        &self,
        session_id: &str,
        message_id: &str,
        _content: &str,
    ) -> Result<(), String> {
        if self.fail_publish.load(Ordering::SeqCst) {
            return Err("ui_bus_down".into());
        }
        self.published
            .lock()
            .unwrap()
            .push((session_id.to_string(), message_id.to_string()));
        Ok(())
    }
}
fn host(summarizer: &Arc<Fake>) -> Host {
    Host {
        summarizer: summarizer.clone(),
        parent_lock: Arc::new(tokio::sync::Mutex::new(())),
        published: Mutex::new(Vec::new()),
        fail_publish: AtomicBool::new(false),
    }
}
const NO_WAIT: DriveOptions = DriveOptions {
    append_retries: 0,
    retry_delay: Duration::from_millis(0),
};

fn setup() -> test_env::SandboxGuard {
    let sandbox = test_env::sandbox();
    let conn = get_connection().unwrap();
    crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_messages (
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL DEFAULT '',
            tool_name TEXT, tool_call_id TEXT, tool_input TEXT, tool_output TEXT, model TEXT,
            sequence INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, images TEXT,
            compact_from_sequence INTEGER, compact_tokens_before INTEGER, compact_tokens_after INTEGER,
            tool_is_error INTEGER NOT NULL DEFAULT 0);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_sessions(session_id,name,session_type,status,model,account_id,created_at,updated_at)
         VALUES(?1,'main','agent','idle','fake/model-1','acct-1','2026-10-12T00:00:00Z','2026-10-12T00:00:00Z')",
        [PARENT],
    )
    .unwrap();
    sandbox
}

fn put(session: &str, id: &str, role: &str, content: &str) -> i64 {
    let conn = get_connection().unwrap();
    let seq = max_sequence(&conn, session).unwrap() + 1;
    conn.execute(
        "INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at,tool_is_error) VALUES(?1,?2,?3,?4,?5,?6,0)",
        params![id, session, role, content, seq, format!("2026-10-12T00:00:{:02}Z", seq % 60)],
    )
    .unwrap();
    seq
}
fn put_tool_call(session: &str, id: &str, call: &str) {
    let conn = get_connection().unwrap();
    let seq = max_sequence(&conn, session).unwrap() + 1;
    conn.execute(
        "INSERT INTO agent_messages(id,session_id,role,content,tool_name,tool_call_id,tool_input,sequence,created_at,tool_is_error)
         VALUES(?1,?2,'tool_call','Tool call: read','read_file',?3,'{}',?4,'2026-10-12T00:01:00Z',0)",
        params![id, session, call, seq],
    )
    .unwrap();
}
fn put_tool_result(session: &str, id: &str, call: &str) {
    let conn = get_connection().unwrap();
    let seq = max_sequence(&conn, session).unwrap() + 1;
    conn.execute(
        "INSERT INTO agent_messages(id,session_id,role,content,tool_name,tool_call_id,tool_output,sequence,created_at,tool_is_error)
         VALUES(?1,?2,'tool_result','ok','read_file',?3,'ok',?4,'2026-10-12T00:01:01Z',0)",
        params![id, session, call, seq],
    )
    .unwrap();
}

fn big_history() -> String {
    "旧上下文细节：".to_string() + &"夜间分片重建在索引压缩完成之后执行。".repeat(40)
}

fn seed_parent() -> String {
    put(
        PARENT,
        "p-u1",
        "user",
        &format!("目标：整理账单导出。{}", big_history()),
    );
    put(PARENT, "p-a1", "assistant", &big_history());
    put(
        PARENT,
        "p-u2",
        "user",
        &format!("再看重试策略。{}", big_history()),
    );
    put(PARENT, "p-a2", "assistant", &big_history());
    "p-a2".to_string()
}

fn anchor_for(session: &str, message: &str) -> DurableAnchor {
    let conn = get_connection().unwrap();
    let revision = max_sequence(&conn, session).unwrap() as u64;
    resolve_with_connection(
        &conn,
        session,
        session,
        &format!("session:{session}"),
        message,
        revision,
    )
    .unwrap()
}

fn req(intent: &str, anchor: DurableAnchor) -> CreateTaskForkRequest {
    CreateTaskForkRequest {
        intent_id: intent.into(),
        name: "对账导出重试".into(),
        anchor,
    }
}

fn count(sql: &str, session: &str) -> i64 {
    get_connection()
        .unwrap()
        .query_row(sql, [session], |r| r.get(0))
        .unwrap()
}

fn parent_rows_snapshot() -> Vec<(String, i64, String, String)> {
    let conn = get_connection().unwrap();
    load_rows(&conn, PARENT, None)
        .unwrap()
        .into_iter()
        .map(|r| (r.id, r.sequence, r.role, r.content))
        .collect()
}

// ───────────────────────── create ─────────────────────────

#[tokio::test]
async fn create_compresses_then_publishes_independent_child_with_only_the_seed() {
    let _s = setup();
    let anchor_id = seed_parent();
    let fake = Fake::new(Mode::Ok("压缩摘要：目标=账单导出；决定=指数退避。".into()));
    let anchor = anchor_for(PARENT, &anchor_id);
    let view = create_task_fork(PARENT, req("intent-1", anchor), fake.as_ref())
        .await
        .unwrap();

    assert!(view.newly_published);
    assert_eq!(view.link.state, "active");
    let child = view.link.child_session_id.clone().unwrap();
    assert_ne!(child, PARENT);
    assert_eq!(*fake.last_kind.lock().unwrap(), Some(SummaryKind::Compress));
    // Child holds exactly the compressed seed — none of the parent's raw rows.
    let rows = load_rows(&get_connection().unwrap(), &child, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].content.starts_with(SEED_MESSAGE_PREFIX));
    assert!(rows[0].content.contains("指数退避"));
    assert!(!rows[0].content.contains("旧上下文细节"));
    // Task input is smaller than the full history it was compressed from.
    assert!(
        view.link.seed_tokens.unwrap() < view.link.source_tokens,
        "{:?}",
        (view.link.seed_tokens, view.link.source_tokens)
    );
    // Ownership + anchor + provenance are durable.
    assert_eq!(view.link.parent_session_id, PARENT);
    assert_eq!(view.link.anchor_message_id, "p-a2");
    assert_eq!(view.link.anchor_sequence, 3);
    assert_eq!(view.link.model_id, "fake/model-1");
    // The child session row inherits the parent's model/account and links to it.
    let (model, account, parent): (String, String, String) = get_connection()
        .unwrap()
        .query_row(
            "SELECT model,account_id,parent_session_id FROM agent_sessions WHERE session_id=?1",
            [&child],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (model.as_str(), account.as_str(), parent.as_str()),
        ("fake/model-1", "acct-1", PARENT)
    );
    // The summarizer saw the visible parent transcript, not child rows.
    assert!(fake
        .last_transcript
        .lock()
        .unwrap()
        .contains("旧上下文细节"));
}

#[tokio::test]
async fn create_failure_publishes_nothing_and_never_falls_back_to_full_history() {
    let _s = setup();
    let anchor_id = seed_parent();
    let fake = Fake::new(Mode::Err("provider_down".into()));
    let anchor = anchor_for(PARENT, &anchor_id);
    let e = create_task_fork(PARENT, req("intent-2", anchor.clone()), fake.as_ref())
        .await
        .unwrap_err();
    assert!(e.contains("provider_down"));
    let (id, child) = deterministic_ids(PARENT, "intent-2");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_sessions WHERE session_id LIKE ?1 || '-task-%'",
            PARENT
        ),
        0
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",
            &child
        ),
        0
    );
    let link = view_by_id(&id).unwrap().link;
    assert_eq!(link.state, "compress_failed");
    assert!(link.error.unwrap().contains("provider_down"));
    assert!(link.child_session_id.is_none());

    // Empty output is also a failure.
    fake.set(Mode::Ok("   ".into()));
    assert!(
        create_task_fork(PARENT, req("intent-2", anchor.clone()), fake.as_ref())
            .await
            .is_err()
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",
            &child
        ),
        0
    );

    // Explicit retry with the same intent recovers with the same ids, then replays idempotently.
    fake.set(Mode::Ok("恢复后的摘要".into()));
    let v = create_task_fork(PARENT, req("intent-2", anchor.clone()), fake.as_ref())
        .await
        .unwrap();
    assert!(v.newly_published);
    assert_eq!(v.link.child_session_id.as_deref(), Some(child.as_str()));
    let calls = fake.calls();
    let replay = create_task_fork(PARENT, req("intent-2", anchor), fake.as_ref())
        .await
        .unwrap();
    assert!(!replay.newly_published);
    assert_eq!(fake.calls(), calls, "replay must not call the model again");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",
            &child
        ),
        1
    );
}

#[tokio::test]
async fn create_rejects_foreign_anchor_stale_anchor_and_unpaired_tool_prefix() {
    let _s = setup();
    let anchor_id = seed_parent();
    let fake = Fake::new(Mode::Ok("x".into()));
    let mut foreign = anchor_for(PARENT, &anchor_id);
    foreign.owner_session_id = "sde-other".into();
    assert_eq!(
        create_task_fork(PARENT, req("i-foreign", foreign), fake.as_ref())
            .await
            .unwrap_err(),
        "task_fork_anchor_owner_mismatch"
    );

    let stale = anchor_for(PARENT, &anchor_id);
    put(PARENT, "p-u3", "user", "主线继续");
    let e = create_task_fork(PARENT, req("i-stale", stale), fake.as_ref())
        .await
        .unwrap_err();
    assert!(e.contains("journey_anchor_changed"), "{e}");

    put_tool_call(PARENT, "p-tc", "call-1");
    let tool_anchor = anchor_for(PARENT, "p-tc");
    let e = create_task_fork(PARENT, req("i-tool", tool_anchor), fake.as_ref())
        .await
        .unwrap_err();
    assert_eq!(e, "task_fork_unpaired_tool_prefix");
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
async fn parent_keeps_running_while_compression_is_in_flight() {
    let _s = setup();
    let anchor_id = seed_parent();
    let fake = Fake::new(Mode::Gate);
    let anchor = anchor_for(PARENT, &anchor_id);
    let task = {
        let fake = fake.clone();
        tokio::spawn(
            async move { create_task_fork(PARENT, req("i-live", anchor), fake.as_ref()).await },
        )
    };
    fake.entered.notified().await;
    // Main thread continues (no lock held during compression).
    put(PARENT, "p-u3", "user", "主线在压缩期间继续");
    put(PARENT, "p-a3", "assistant", "主线回答");
    fake.release.notify_one();
    let view = task.await.unwrap().unwrap();
    assert_eq!(view.link.state, "active");
    assert_eq!(
        view.link.anchor_sequence, 3,
        "fork stays at the frozen anchor, not the new tail"
    );
    // The new main rows were not part of the child's source.
    let child = view.link.child_session_id.unwrap();
    let rows = load_rows(&get_connection().unwrap(), &child, None).unwrap();
    assert!(!rows[0].content.contains("主线在压缩期间继续"));
}

#[tokio::test]
async fn publish_fails_closed_when_the_frozen_prefix_was_rewritten() {
    let _s = setup();
    let anchor_id = seed_parent();
    let fake = Fake::new(Mode::Gate);
    let anchor = anchor_for(PARENT, &anchor_id);
    let task = {
        let fake = fake.clone();
        tokio::spawn(async move {
            create_task_fork(PARENT, req("i-rewrite", anchor), fake.as_ref()).await
        })
    };
    fake.entered.notified().await;
    get_connection()
        .unwrap()
        .execute(
            "UPDATE agent_messages SET content='被改写' WHERE id='p-u1'",
            [],
        )
        .unwrap();
    fake.release.notify_one();
    let e = task.await.unwrap().unwrap_err();
    assert_eq!(e, "task_fork_parent_prefix_changed");
    let (id, child) = deterministic_ids(PARENT, "i-rewrite");
    assert_eq!(view_by_id(&id).unwrap().link.state, "compress_failed");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",
            &child
        ),
        0
    );
}

// ───────────────────────── finish ─────────────────────────

async fn published(intent: &str, fake: &Arc<Fake>) -> (TaskForkLink, String) {
    let anchor_id = seed_parent();
    let anchor = anchor_for(PARENT, &anchor_id);
    let v = create_task_fork(PARENT, req(intent, anchor), fake.as_ref())
        .await
        .unwrap();
    let child = v.link.child_session_id.clone().unwrap();
    put(&child, "c-u1", "user", "Task 内：请实现指数退避，最多 5 次");
    put(
        &child,
        "c-a1",
        "assistant",
        "Task 内：已实现 backoff(1s,2s,4s,8s,16s)，文件 retry.rs",
    );
    (v.link, child)
}

fn finish_req(id: &str) -> FinishTaskForkRequest {
    FinishTaskForkRequest {
        task_fork_id: id.into(),
        outcome: "completed".into(),
    }
}

#[tokio::test]
async fn finish_appends_exactly_one_source_identified_summary_and_keeps_the_prefix() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("压缩摘要".into()));
    let (link, child) = published("f-1", &fake).await;
    let before = parent_rows_snapshot();
    fake.set(Mode::Ok("结论：已实现指数退避，见 retry.rs。".into()));
    let h = host(&fake);

    let (v, start) = begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    assert!(start);
    assert_eq!(v.job.as_ref().unwrap().state, "queued");
    let done = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    assert_eq!(done.link.state, "finished");
    let job = done.job.unwrap();
    assert_eq!(job.state, "appended");
    assert_eq!(
        job.parent_message_id.as_deref(),
        Some(format!("task-summary:{}", link.id).as_str())
    );
    assert_eq!(job.appended_after_message_id.as_deref(), Some("p-a2"));
    assert_eq!(job.parent_sequence, Some(4));
    assert_eq!(job.parent_prefix_hash_before, job.parent_prefix_hash_after);
    assert_eq!(*fake.last_kind.lock().unwrap(), Some(SummaryKind::Finish));

    // Old parent rows are byte-identical; exactly one new row at the tail.
    let after = parent_rows_snapshot();
    assert_eq!(&after[..before.len()], &before[..]);
    assert_eq!(after.len(), before.len() + 1);
    let last = after.last().unwrap();
    assert_eq!((last.2.as_str(), last.1), ("user", 4));
    assert!(last.3.starts_with(SUMMARY_MESSAGE_PREFIX));
    assert!(last.3.contains(&link.id) && last.3.contains(&child) && last.3.contains("p-a2"));
    assert!(last.3.contains("指数退避"));
    // No full Task transcript is injected into the main line.
    assert!(!last.3.contains("Task 内：请实现指数退避"));
    assert!(!after.iter().any(|r| r.3.contains("Task 内：已实现")));
    // Frozen anchor-prefix hash still matches the creation-time hash.
    let anchor_rows = load_rows(
        &get_connection().unwrap(),
        PARENT,
        Some(link.anchor_sequence),
    )
    .unwrap();
    assert_eq!(rows_hash(&anchor_rows), link.parent_prefix_hash);
}

#[tokio::test]
async fn duplicate_finish_retry_and_recovery_never_append_twice() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-2", &fake).await;
    fake.set(Mode::Ok("结论一".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let calls = fake.calls();
    for _ in 0..3 {
        let (v, start) = begin_finish(PARENT, &finish_req(&link.id)).unwrap();
        assert!(!start, "finished task must not restart a driver");
        assert_eq!(v.link.state, "finished");
        drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    }
    assert_eq!(recover_pending(&h, NO_WAIT).await, 0);
    assert_eq!(fake.calls(), calls);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
    // Direct re-append is a no-op error, never a second row.
    assert!(matches!(
        append_summary_to_parent(&link.id),
        Err(AppendError::NotReady)
    ));
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
}

#[tokio::test]
async fn summary_failure_keeps_the_task_and_is_explicitly_retryable() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, child) = published("f-3", &fake).await;
    fake.set(Mode::Err("summary_provider_503".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let job = v.job.unwrap();
    assert_eq!(job.state, "failed");
    assert!(job.error.unwrap().contains("summary_provider_503"));
    assert_eq!(v.link.state, "finishing");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );
    // Task child is untouched and recovery does not silently retry a failed job.
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",
            &child
        ),
        3
    );
    assert_eq!(recover_pending(&h, NO_WAIT).await, 0);

    fake.set(Mode::Ok("重试后的结论".into()));
    let (_, start) = begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    assert!(start);
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    assert_eq!(v.job.as_ref().unwrap().state, "appended");
    assert_eq!(v.job.unwrap().attempts, 2);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
}

#[tokio::test]
async fn cancel_discards_a_late_summary_and_the_task_can_finish_again() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-4", &fake).await;
    fake.set(Mode::Gate);
    let h = Arc::new(host(&fake));
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    let driver = {
        let h = h.clone();
        let id = link.id.clone();
        tokio::spawn(async move { drive_finish_job(h.as_ref(), &id, NO_WAIT).await })
    };
    fake.entered.notified().await;
    let cancelled = cancel_finish(PARENT, &link.id).unwrap();
    assert_eq!(cancelled.link.state, "active");
    assert_eq!(cancelled.job.unwrap().state, "cancelled");
    fake.release.notify_one();
    let v = driver.await.unwrap().unwrap();
    assert_eq!(
        v.job.unwrap().state,
        "cancelled",
        "late result must be dropped by the lease CAS"
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );

    fake.set(Mode::Ok("第二次结论".into()));
    let (_, start) = begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    assert!(start);
    let v = drive_finish_job(h.as_ref(), &link.id, NO_WAIT)
        .await
        .unwrap();
    assert_eq!(v.job.unwrap().state, "appended");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
    // An appended summary can no longer be cancelled.
    assert_eq!(
        cancel_finish(PARENT, &link.id).unwrap_err(),
        "task_fork_already_appended"
    );
}

#[tokio::test]
async fn parent_busy_and_open_tool_call_defer_then_append_once_after_recovery() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-5", &fake).await;
    fake.set(Mode::Ok("延迟归并的结论".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();

    // Parent turn running → summary is durable, append deferred.
    let held = h.parent_lock.clone().try_lock_owned().unwrap();
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let job = v.job.unwrap();
    assert_eq!(job.state, "summarized");
    assert_eq!(job.deferred_reason.as_deref(), Some("parent_busy"));
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );
    drop(held);

    // Parent mid tool-call → still deferred; legality of tool pairing is preserved.
    put(PARENT, "p-u3", "user", "继续");
    put_tool_call(PARENT, "p-tc2", "call-2");
    let calls = fake.calls();
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    assert_eq!(
        v.job.unwrap().deferred_reason.as_deref(),
        Some("parent_tool_call_open")
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );

    put_tool_result(PARENT, "p-tr2", "call-2");
    put(PARENT, "p-a3", "assistant", "工具完成");
    // "Restart": fresh recovery sweep, no model call needed for the append stage.
    assert_eq!(recover_pending(&h, NO_WAIT).await, 1);
    assert_eq!(
        fake.calls(),
        calls,
        "append stage must not call the model again"
    );
    let v = view_by_id(&link.id).unwrap();
    let job = v.job.unwrap();
    assert_eq!(job.state, "appended");
    assert_eq!(
        job.appended_after_message_id.as_deref(),
        Some("p-a3"),
        "append position is the live tail"
    );
    assert_eq!(recover_pending(&h, NO_WAIT).await, 0);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
}

#[tokio::test]
async fn rewritten_parent_prefix_refuses_the_merge() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-6", &fake).await;
    fake.set(Mode::Ok("结论".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    get_connection()
        .unwrap()
        .execute("DELETE FROM agent_messages WHERE id='p-a1'", [])
        .unwrap();
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let job = v.job.unwrap();
    assert_eq!(job.state, "summarized");
    assert!(job
        .deferred_reason
        .unwrap()
        .starts_with("task_fork_parent_prefix_changed"));
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );
}

#[tokio::test]
async fn owner_isolation_and_listing() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, child) = published("f-7", &fake).await;
    assert_eq!(
        begin_finish("sde-stranger", &finish_req(&link.id)).unwrap_err(),
        "task_fork_owner_mismatch"
    );
    assert_eq!(
        cancel_finish("sde-stranger", &link.id).unwrap_err(),
        "task_fork_owner_mismatch"
    );
    assert!(list_for_owner("sde-stranger").unwrap().is_empty());
    assert_eq!(list_for_owner(PARENT).unwrap().len(), 1);
    assert_eq!(
        list_for_owner(&child).unwrap().len(),
        1,
        "the child sees its own task"
    );
    // A create under another owner never reuses this intent's link.
    put("sde-stranger-seed", "x", "user", "x");
    assert_eq!(
        begin_finish(
            &child,
            &FinishTaskForkRequest {
                task_fork_id: link.id.clone(),
                outcome: "bogus".into()
            }
        )
        .unwrap_err(),
        "task_fork_invalid_outcome"
    );
}

#[tokio::test]
async fn stale_running_job_is_taken_over_exactly_once() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-8", &fake).await;
    fake.set(Mode::Ok("接管后的结论".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    // Simulate a crash mid-run: durable `running` with no live driver.
    get_connection()
        .unwrap()
        .execute(
            "UPDATE task_fork_summary_jobs SET state='running',lease_token='dead',attempts=1",
            [],
        )
        .unwrap();
    assert_eq!(recover_pending(&h, NO_WAIT).await, 1);
    let v = view_by_id(&link.id).unwrap();
    assert_eq!(v.job.as_ref().unwrap().state, "appended");
    assert_eq!(v.job.unwrap().attempts, 2);
    assert_eq!(recover_pending(&h, NO_WAIT).await, 0);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
}

struct NoRuntimeHost;
#[async_trait::async_trait]
impl TaskForkHost for NoRuntimeHost {
    async fn summarizer_for_finish(
        &self,
        _link: &TaskForkLink,
    ) -> Result<Arc<dyn TaskForkSummarizer>, String> {
        Err("task_fork_runtime_unavailable: 子会话与主线都没有活动运行时".into())
    }
    async fn try_lock_parent(&self, _p: &str) -> Result<ParentLockGuard, String> {
        Ok(ParentLockGuard::unregistered())
    }
    async fn publish_message_event(&self, _s: &str, _m: &str, _c: &str) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn missing_runtime_after_restart_requeues_instead_of_failing_and_never_switches_model() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-9", &fake).await;
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    let v = drive_finish_job(&NoRuntimeHost, &link.id, NO_WAIT)
        .await
        .unwrap();
    let job = v.job.unwrap();
    assert_eq!(job.state, "queued", "no runtime is deferral, not failure");
    assert_eq!(job.attempts, 0);
    assert!(job
        .deferred_reason
        .unwrap()
        .starts_with("task_fork_runtime_unavailable"));
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        0
    );
    // Once a runtime is active the same sweep completes it, exactly once.
    fake.set(Mode::Ok("运行时恢复后的结论".into()));
    let h = host(&fake);
    assert_eq!(recover_pending(&h, NO_WAIT).await, 1);
    assert_eq!(view_by_id(&link.id).unwrap().job.unwrap().state, "appended");
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
}

#[tokio::test]
async fn ui_event_is_published_once_and_healed_after_a_bus_failure() {
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-10", &fake).await;
    fake.set(Mode::Ok("结论".into()));
    let h = host(&fake);
    h.fail_publish.store(true, Ordering::SeqCst);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    let v = drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let job = v.job.unwrap();
    assert_eq!(
        job.state, "appended",
        "the durable append does not depend on the UI bus"
    );
    assert!(job.event_published_at.is_none());
    assert!(h.published.lock().unwrap().is_empty());
    // Bus recovers: the sweep publishes the event, once, without touching the transcript again.
    h.fail_publish.store(false, Ordering::SeqCst);
    assert_eq!(recover_pending(&h, NO_WAIT).await, 1);
    assert_eq!(
        h.published.lock().unwrap().as_slice(),
        &[(PARENT.to_string(), format!("task-summary:{}", link.id))]
    );
    assert!(view_by_id(&link.id)
        .unwrap()
        .job
        .unwrap()
        .event_published_at
        .is_some());
    assert_eq!(recover_pending(&h, NO_WAIT).await, 0);
    assert_eq!(h.published.lock().unwrap().len(), 1);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'",
            PARENT
        ),
        1
    );
    // The seed payload for the child is derivable for UI publication.
    let (child, seed_id, content) = seed_event_payload(&view_by_id(&link.id).unwrap()).unwrap();
    assert_eq!(seed_id, format!("task-seed:{}", link.id));
    assert!(content.starts_with(SEED_MESSAGE_PREFIX) && child.contains("-task-"));
}

#[tokio::test]
async fn summary_joins_the_journey_branch_membership_and_stays_provider_visible() {
    use crate::core::session::journey_application_service::{
        CreateTaskRequest, SessionJourneyApplicationService, TaskStartPosition,
    };
    use crate::core::session::journey_lifecycle::{SqliteJourneyRepository, TaskState};
    let _s = setup();
    let fake = Fake::new(Mode::Ok("c".into()));
    let (link, _) = published("f-11", &fake).await;
    // The parent also uses a Journey with a pending "next user message" task and memberships.
    {
        let mut conn = get_connection().unwrap();
        SessionJourneyApplicationService::create_task(
            &mut conn,
            CreateTaskRequest {
                session_id: PARENT.into(),
                expected_revision: 0,
                task_id: "jt".into(),
                name: "journey".into(),
                position: TaskStartPosition::最近用户消息,
            },
        )
        .unwrap();
        let rows = load_rows(&conn, PARENT, None).unwrap();
        let tx = conn.transaction().unwrap();
        for row in &rows {
            crate::core::session::journey_runtime_persistence::assign_inserted_message(
                &tx,
                PARENT,
                &row.id,
                row.sequence,
                &row.role,
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    fake.set(Mode::Ok("Journey 下的结论".into()));
    let h = host(&fake);
    begin_finish(PARENT, &finish_req(&link.id)).unwrap();
    drive_finish_job(&h, &link.id, NO_WAIT).await.unwrap();
    let conn = get_connection().unwrap();
    let msg = format!("task-summary:{}", link.id);
    let branch: String = conn
        .query_row("SELECT branch_id FROM session_journey_memberships WHERE session_id=?1 AND message_id=?2", params![PARENT, msg], |r| r.get(0))
        .unwrap();
    assert_eq!(branch, "main");
    let history = crate::core::session::persistence::load_llm_history(PARENT).unwrap();
    assert!(
        history.last().unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("Journey 下的结论"),
        "summary must reach the provider view"
    );
    let journey = SqliteJourneyRepository::load(&conn, PARENT)
        .unwrap()
        .unwrap();
    assert_eq!(journey.tasks["jt"].state, TaskState::Active);
}
