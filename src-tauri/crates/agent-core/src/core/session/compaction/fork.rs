//! Compact-fork primitive.
//!
//! When a *channel-attached* session triggers context compaction, we don't
//! want to keep mutating the same `session_id` — the chat user expects each
//! conversation thread to be addressable and the long-running session can
//! become a graveyard of compacted summaries. Instead we **fork**: persist
//! the compacted transcript into a *new* session whose `parent_session_id`
//! points at the old one, archive the old session, and rebind the chat's
//! `gateway_bindings` row to the new id. The user sees a one-line notice on
//! the next outbound message.
//!
//! Hermes parallel:
//! - Auto-compact fork: `hermes-agent/run_agent.py:7240-7301`
//!   (`end_session` → `create_session(parent_session_id=...)` →
//!   `rewrite_transcript(...)`).
//! - Manual `/compress`: `hermes-agent/gateway/run.py:6226-6321`
//!   (same plus `session_entry.session_id = new_id` to flip the binding).
//!
//! App-side (IDE-launched) sessions retain the existing in-place compact
//! behaviour — see `processor.rs:812-907`. The decision branch is
//! `BindingStore::find_by_target(session_id).is_some()`: a binding hit
//! means the session is owned by an external chat and must fork; no
//! binding means an app session and we keep the in-place semantics.
//!
//! # Failure mode
//!
//! If any step of the fork fails (DB write, binding update, etc.), the
//! caller falls back to in-place compaction so the user's turn still goes
//! through. The fork is best-effort — Hermes does the same: a failed
//! `create_session` falls back to logging and continuing the existing one.

use tracing::{info, warn};

use crate::integrations::gateway::ResetPolicy;
use crate::state::AgentAppState;

use super::super::persistence as unified_persistence;
use super::super::session_id::{next_version_for, with_version};
use super::super::SessionStatus;

/// Outcome of `attempt_fork`.
#[derive(Debug)]
pub enum ForkOutcome {
    /// Fork succeeded — caller must abort the current turn and re-dispatch
    /// the inbound message against `new_session_id`. The compacted
    /// transcript has already been persisted under the new id; the old
    /// session row is archived and the binding now points at the new id.
    Forked { new_session_id: String },
    /// Session is not channel-attached (no binding pointing at it). Caller
    /// keeps the in-place compaction path.
    NotChannelAttached,
    /// A fork was attempted but a step failed; caller falls back to
    /// in-place compaction so the user's turn still completes. The
    /// attached `reason` is also logged at `warn` level — keep it short
    /// and human-readable (e.g. `"next_version_for: sqlite locked"`).
    /// Exposed via the debug endpoint so E2E can assert on the
    /// failure mode without scraping server logs.
    Failed(String),
}

/// Inputs for `attempt_fork`. Bundled so the call site in
/// `processor.rs` stays terse.
pub struct ForkInputs<'a> {
    pub state: &'a AgentAppState,
    /// Compacted in-memory transcript (output of `ContextCompactor::compact`
    /// after `post_compact_cleanup`). The fork persists THIS into the new
    /// session, not the pre-compact transcript.
    pub compacted_messages: &'a [serde_json::Value],
    /// Old session id (the one currently being processed).
    pub old_session_id: &'a str,
    /// Reset policy is consulted only for `notify` flag — we honour the
    /// same user-visibility setting as idle-reset.
    pub reset_policy: &'a ResetPolicy,
    pub weighted_checkpoint: Option<&'a crate::model_context::weighted_usage::WeightedCheckpoint>,
}

/// Detect whether `session_id` is owned by a gateway binding and, if so,
/// perform the fork: persist compacted transcript under a new versioned
/// id, archive the old session, rebind the chat, queue notify.
///
/// Caller is responsible for aborting the current turn on `Forked` so the
/// user message gets re-dispatched against the new id (see
/// `ProcessingResult::fork_redirect`).
pub async fn attempt_fork(inputs: ForkInputs<'_>) -> ForkOutcome {
    let ForkInputs {
        state,
        compacted_messages,
        old_session_id,
        reset_policy,
        weighted_checkpoint,
    } = inputs;

    // 1. Detect channel attachment via reverse lookup.
    let Some(binding) = state.gateway_bindings.find_by_target(old_session_id).await else {
        return ForkOutcome::NotChannelAttached;
    };

    // 2. Mint the next versioned id under the same base.
    //
    // Strip a trailing `-vN` from `old_session_id` to recover the base.
    // Example: `osagent-telegram-42-v3` → base `osagent-telegram-42` → v4.
    // For first-time fork (`osagent-telegram-42`) the base IS the session id.
    //
    // `next_version_for` scans `agent_sessions` for existing rows. The
    // caller's `old_session_id` is, by `attempt_fork`'s contract, a live
    // in-memory session — it may or may not be committed to the DB yet
    // (see #p61: a session can be in the in-memory runtime pool but
    // still have an async pending `upsert_session`). We therefore treat
    // `old_session_id` as implicitly existing regardless of whether the
    // DB scan sees it, so `next_v >= old_v + 1` is guaranteed.
    let base = strip_version_suffix(old_session_id);
    let scanned_next_v = match tokio::task::spawn_blocking({
        let base = base.to_string();
        move || next_version_for(&base)
    })
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(err)) => {
            let reason = format!("next_version_for({}): {}", base, err);
            warn!("[compact_fork] {} — falling back to in-place", reason);
            return ForkOutcome::Failed(reason);
        }
        Err(err) => {
            let reason = format!("next_version_for join error: {}", err);
            warn!("[compact_fork] {} — falling back to in-place", reason);
            return ForkOutcome::Failed(reason);
        }
    };
    // Raise `next_v` past the caller's current version. The caller's
    // version is `1` when `old_session_id == base`, else parsed from
    // the trailing `-vN`. This avoids a race where `old_session_id`
    // hasn't yet been flushed to `agent_sessions` (e.g. E2E driving
    // fork immediately after session creation — the row is pending an
    // async write and the DB scan returns 0 rows).
    let old_v: u32 = if old_session_id == base {
        1
    } else {
        old_session_id
            .rsplit_once("-v")
            .and_then(|(_, tail)| tail.parse::<u32>().ok())
            .unwrap_or(1)
    };
    let next_v = scanned_next_v.max(old_v + 1);
    let new_session_id = with_version(base, next_v);
    #[cfg(test)]
    if old_session_id == "osagent-race-old" {
        static RACE: std::sync::OnceLock<tokio::sync::Barrier> = std::sync::OnceLock::new();
        RACE.get_or_init(|| tokio::sync::Barrier::new(2)).wait().await;
    }

    if new_session_id == old_session_id {
        let reason = format!(
            "computed new_session_id equals old ({}): base={:?}, scanned_next_v={}, old_v={}, next_v={}",
            old_session_id, base, scanned_next_v, old_v, next_v
        );
        warn!("[compact_fork] {}", reason);
        return ForkOutcome::Failed(reason);
    }

    // 3. Load old session record for parent metadata copy. If the row
    // hasn't been persisted yet (race with create-and-route: the binding
    // landed but `upsert_session` is still pending on a blocking
    // task), fall back to a minimal record synthesised from the
    // binding's `SessionKey`. The `channel` / `chat_id` on the
    // `SessionKey` are our source of truth for user-visible
    // addressing; `model` / `agent_definition_id` / `name` are left
    // as `None` and the re-dispatched message will refill them the
    // first time the new session boots.
    let old_record = match tokio::task::spawn_blocking({
        let sid = old_session_id.to_string();
        move || unified_persistence::get_session(&sid)
    })
    .await
    {
        Ok(Ok(Some(rec))) => rec,
        Ok(Ok(None)) => {
            warn!(
                "[compact_fork] get_session({}): no row — synthesising minimal record \
                 from binding (race: session row not yet flushed)",
                old_session_id
            );
            // `SessionKey` wraps the wire form `{channel}:{chat_id}`
            // (with an optional trailing `:{sender_id}` when
            // `group_sessions_per_user` is on). The first two fields
            // are what `prepend_reset_notice` keys on, so we carve
            // them off here.
            let raw = &binding.session_key.0;
            let mut parts = raw.splitn(3, ':');
            let channel = parts.next().unwrap_or("").to_string();
            let chat_id = parts.next().unwrap_or("").to_string();
            unified_persistence::UnifiedSessionRecord {
                session_id: old_session_id.to_string(),
                channel: if channel.is_empty() {
                    None
                } else {
                    Some(channel)
                },
                chat_id: if chat_id.is_empty() {
                    None
                } else {
                    Some(chat_id)
                },
                ..Default::default()
            }
        }
        Ok(Err(err)) => {
            let reason = format!("get_session({}): {}", old_session_id, err);
            warn!("[compact_fork] {}", reason);
            return ForkOutcome::Failed(reason);
        }
        Err(err) => {
            let reason = format!("get_session join error: {}", err);
            warn!("[compact_fork] {}", reason);
            return ForkOutcome::Failed(reason);
        }
    };

    // 4. Insert new session row with parent linkage. Inherits the full
    // identity quartet from the parent — see `build_forked_record` for
    // the contract.
    let now = chrono::Utc::now().to_rfc3339();
    let mut rec = build_forked_record(&old_record, &new_session_id, old_session_id, &now);
    rec.status = SessionStatus::Archived.as_str().to_string();
    let upsert_result =
        tokio::task::spawn_blocking(move || unified_persistence::insert_fresh_session(&rec)).await;
    match upsert_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            let reason = format!("upsert_session({}): {}", new_session_id, err);
            warn!("[compact_fork] {}", reason);
            return ForkOutcome::Failed(reason);
        }
        Err(err) => {
            let reason = format!("upsert_session join error: {}", err);
            warn!("[compact_fork] {}", reason);
            return ForkOutcome::Failed(reason);
        }
    }

    // 5. Persist compacted transcript under new session id. This must retain
    // the compact summary `system` row; `save_subagent_transcript` intentionally
    // skips system prompts and would lose the durable compact boundary.
    // `seed_session_with_messages` refuses non-empty targets, so a fork can
    // never clobber an existing transcript.
    let persist_messages = compacted_messages.to_vec();
    let persist_result = tokio::task::spawn_blocking({
        let sid = new_session_id.clone();
        move || unified_persistence::seed_session_with_messages(&sid, &persist_messages)
    })
    .await;
    match persist_result {
        Ok(Ok(())) => {
            #[cfg(test)]
            if old_session_id == "osagent-race-old" {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        Ok(Err(err)) => {
            let reason = format!("seed_session_with_messages({}): {}", new_session_id, err);
            warn!(
                "[compact_fork] {} — old session still active; falling back to in-place",
                reason
            );
            // This reservation is already archived. Preserve it rather than
            // deleting usage/history that a late writer may have attached.
            return ForkOutcome::Failed(reason);
        }
        Err(err) => {
            let reason = format!("seed_session_with_messages join error: {}", err);
            warn!("[compact_fork] {}", reason);
            return ForkOutcome::Failed(reason);
        }
    }

    match state.gateway_bindings.publish_fork_with_checkpoint(binding.session_key.clone(), old_session_id.to_string(), new_session_id.clone(), weighted_checkpoint.cloned()).await {
        Ok(true) => {}
        Ok(false) => return ForkOutcome::Failed("binding changed during fork; reserved child remains archived".into()),
        Err(err) => return ForkOutcome::Failed(format!("fork publication: {}", err)),
    }
    state.invalidate_session(old_session_id).await;

    // 9. Queue user-facing notice (delivered as prefix on the next
    //    outbound message — same machinery as idle-reset).
    //
    // `prepend_reset_notice` in gateway.rs keys by `{channel}:{chat_id}`
    // (2 parts, not the full session_key which can carry a trailing
    // `:sender_id` when `group_sessions_per_user` is enabled). We derive
    // the 2-part shape from the old session record's channel/chat_id
    // fields so both the "reset" and "fork" notice paths share a key.
    if reset_policy.notify {
        match (&old_record.channel, &old_record.chat_id) {
            (Some(channel), Some(chat_id)) => {
                let key = format!("{}:{}", channel, chat_id);
                let notice = format!(
                    "🗜️ Context compacted. Continuing in new session `{}` (previous: `{}`).",
                    new_session_id, old_session_id
                );
                let mut guard = state.pending_reset_notifies.lock().await;
                guard.insert(key, notice);
            }
            _ => {
                warn!(
                    "[compact_fork] old session {} has no channel/chat_id — skipping notice",
                    old_session_id
                );
            }
        }
    }

    info!(
        "[compact_fork] forked {} → {} (binding rebound, parent linked, old archived)",
        old_session_id, new_session_id
    );

    ForkOutcome::Forked { new_session_id }
}

/// Build the new `UnifiedSessionRecord` written by a successful fork.
///
/// Pure helper extracted from `attempt_fork` so the inheritance contract
/// can be unit-tested without bringing up `AgentAppState` + a real DB.
///
/// Identity inheritance contract: the four billing/runtime-identity fields
/// (`model`, `account_id`, `workspace_path`, `key_source`) MUST round-trip
/// from `old_record` to the new row. The compactor produces a fresh
/// transcript but does not change *who* is paying or *which* code repo
/// the agent is operating against — those carry over so the resumed
/// session bills the same wallet and resolves the same workspace.
/// Skipping `account_id` / `key_source` was the original split-brain
/// shape: a hosted_key parent forking into an own_key child mis-billed
/// the post-compaction transcript to the user's BYOK wallet (or vice
/// versa). Status starts `Idle` — re-dispatched message flips it to
/// `Running` via the normal pipeline.
fn build_forked_record(
    old_record: &unified_persistence::UnifiedSessionRecord,
    new_session_id: &str,
    old_session_id: &str,
    now: &str,
) -> unified_persistence::UnifiedSessionRecord {
    unified_persistence::UnifiedSessionRecord {
        session_id: new_session_id.to_string(),
        name: old_record.name.clone(),
        status: SessionStatus::Idle.as_str().to_string(),
        model: old_record.model.clone(),
        account_id: old_record.account_id.clone(),
        credential_source: old_record.credential_source.clone(),
        workspace_path: old_record.workspace_path.clone(),
        session_type: old_record.session_type.clone(),
        channel: old_record.channel.clone(),
        chat_id: old_record.chat_id.clone(),
        user_input: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        parent_session_id: Some(old_session_id.to_string()),
        agent_definition_id: old_record.agent_definition_id.clone(),
        key_source: old_record.key_source,
        ..Default::default()
    }
}

/// Strip a trailing `-v{n}` suffix from a session id. Returns the input
/// unchanged when no suffix matches.
///
/// Examples:
/// - `osagent-telegram-42` → `osagent-telegram-42`
/// - `osagent-telegram-42-v2` → `osagent-telegram-42`
/// - `osagent-telegram-42-v17` → `osagent-telegram-42`
/// - `osagent-foo-v` → `osagent-foo-v` (no digits — not a version suffix)
fn strip_version_suffix(sid: &str) -> &str {
    if let Some(idx) = sid.rfind("-v") {
        let tail = &sid[idx + 2..];
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            return &sid[..idx];
        }
    }
    sid
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::key_source::KeySource;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_compact_forks_have_one_durable_winner() {
        let _sandbox = test_helpers::test_env::sandbox();
        let conn = database::db::get_connection().unwrap();
        crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
        unified_persistence::init(&conn).unwrap();
        crate::persistence::test_schema::ensure_session_events_schema(&conn);
        // Use the connection-scoped startup DDL, not one-column placeholder
        // tables: sandbox databases do not rerun the process-global init Once.
        crate::persistence::session_snapshots::ensure_tables_with(&conn).unwrap();
        database::db::init_shell_replay_tables(&conn).unwrap();
        let state = AgentAppState::new();
        state.gateway_bindings.load_from_db().await.unwrap();
        let old = "osagent-race-old";
        unified_persistence::upsert_session(&unified_persistence::UnifiedSessionRecord {
            session_id: old.into(), status: "idle".into(), ..Default::default()
        }).unwrap();
        let key = crate::integrations::gateway::SessionKey("telegram:race".into());
        state.gateway_bindings.set(key.clone(), old.into()).await.unwrap();
        let messages = vec![serde_json::json!({"role":"system","content":"compacted race summary"})];
        let policy = ResetPolicy::default();
        crate::model_context::weighted_usage::record_with_connection(&conn, old, "threshold", 0, 5_000_000_000).unwrap();
        let checkpoint = crate::model_context::weighted_usage::snapshot_with_connection(&conn, old).unwrap();
        crate::model_context::weighted_usage::record_with_connection(&conn, old, "late", 0, 17_000).unwrap();
        let (a,b) = tokio::join!(
            attempt_fork(ForkInputs {state: &state, compacted_messages: &messages, old_session_id: old, reset_policy: &policy, weighted_checkpoint: Some(&checkpoint)}),
            attempt_fork(ForkInputs {state: &state, compacted_messages: &messages, old_session_id: old, reset_policy: &policy, weighted_checkpoint: Some(&checkpoint)})
        );
        eprintln!("actual fork outcomes: {a:?} {b:?}");
        let successes = usize::from(matches!(a, ForkOutcome::Forked {..})) + usize::from(matches!(b, ForkOutcome::Forked {..}));
        let binding = state.gateway_bindings.get(&key).await.unwrap();
        let remainder = crate::model_context::weighted_usage::snapshot_with_connection(&conn, old).unwrap();
        assert_eq!(remainder.milli, 17_000, "fork publication must retire only captured spend");
        assert_eq!(remainder.generation, checkpoint.generation + 1);
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id=?1", [&binding.target_session_id], |r| r.get(0)).unwrap();
        let active_children: i64 = conn.query_row("SELECT COUNT(*) FROM agent_sessions WHERE parent_session_id=?1 AND status != 'archived'", [old], |r| r.get(0)).unwrap();
        assert_eq!(active_children, 1, "active orphan after concurrent fork: {a:?} {b:?}");
        assert_eq!(successes, 1, "two redirects: {a:?} {b:?}");
        assert!(rows > 0, "binding points at missing seeded child: {binding:?}");
        assert!(unified_persistence::get_session(&binding.target_session_id).unwrap().is_some(), "winner child deleted by loser");
        let restarted = crate::integrations::gateway::binding::BindingStore::new();
        restarted.load_from_db().await.unwrap();
        assert_eq!(restarted.get(&key).await.unwrap().target_session_id, binding.target_session_id);
        assert_eq!(unified_persistence::get_session(old).unwrap().unwrap().status, "archived");
        // This runtime path consumes an already-compacted transcript, not a
        // selected Journey endpoint. Verify its real durable replay contract.
        let child = unified_persistence::get_session(&binding.target_session_id).unwrap().unwrap();
        assert_ne!(child.session_id, old);
        assert_eq!(child.parent_session_id.as_deref(), Some(old));
        let replay = unified_persistence::load_llm_history(&child.session_id).unwrap();
        assert!(replay.iter().any(|row| row["content"] == "compacted race summary"));
        assert_eq!(crate::model_context::weighted_usage::snapshot_with_connection(&conn, &child.session_id).unwrap().milli, 0);
        drop(conn);
        for _ in 0..2 {
            let reopened = database::db::get_connection().unwrap();
            assert_eq!(unified_persistence::load_llm_history(&child.session_id).unwrap(), replay);
            assert_eq!(unified_persistence::get_session(&child.session_id).unwrap().unwrap().parent_session_id.as_deref(), Some(old));
            let late = crate::model_context::weighted_usage::snapshot_with_connection(&reopened, old).unwrap();
            assert_eq!(late.milli, 17_000);
            assert_eq!(late.generation, checkpoint.generation + 1);
            assert_eq!(crate::model_context::weighted_usage::snapshot_with_connection(&reopened, &child.session_id).unwrap().milli, 0);
        }
        let conn = database::db::get_connection().unwrap();

        assert!(!state.gateway_bindings.publish_fork(key.clone(), old.into(), "foreign-child".into()).await.unwrap());
        conn.execute_batch("CREATE TRIGGER fail_binding_update BEFORE UPDATE ON gateway_bindings BEGIN SELECT RAISE(ABORT, 'injected binding failure'); END;").unwrap();
        assert!(state.gateway_bindings.set(key.clone(), "must-not-publish".into()).await.is_err());
        assert_eq!(state.gateway_bindings.get(&key).await.unwrap().target_session_id, binding.target_session_id);
        assert!(state.gateway_bindings.publish_fork(key.clone(), binding.target_session_id.clone(), "must-not-publish".into()).await.is_err());
        assert_eq!(unified_persistence::get_session(&binding.target_session_id).unwrap().unwrap().status, "idle");
        restarted.load_from_db().await.unwrap();
        assert_eq!(restarted.get(&key).await.unwrap().target_session_id, binding.target_session_id);
        conn.execute_batch("DROP TRIGGER fail_binding_update;").unwrap();


    }


    #[tokio::test]
    async fn failed_seed_keeps_archived_reservation_and_parent_binding() {
        let _sandbox = test_helpers::test_env::sandbox();
        let conn = database::db::get_connection().unwrap();
        crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
        unified_persistence::init(&conn).unwrap();
        crate::persistence::test_schema::ensure_session_events_schema(&conn);
        crate::persistence::session_snapshots::ensure_tables_with(&conn).unwrap();
        let state = AgentAppState::new();
        state.gateway_bindings.load_from_db().await.unwrap();
        let old = "osagent-failed-seed";
        unified_persistence::upsert_session(&unified_persistence::UnifiedSessionRecord {
            session_id: old.into(), status: "idle".into(), ..Default::default()
        }).unwrap();
        let key = crate::integrations::gateway::SessionKey("telegram:failedseed".into());
        state.gateway_bindings.set(key.clone(), old.into()).await.unwrap();
        conn.execute_batch("CREATE TRIGGER reject_fork_seed BEFORE INSERT ON agent_messages BEGIN SELECT RAISE(ABORT,'seed failure'); END;").unwrap();
        let messages = vec![serde_json::json!({"role":"system","content":"summary"})];
        let policy = ResetPolicy::default();
        assert!(matches!(attempt_fork(ForkInputs {state:&state, compacted_messages:&messages, old_session_id:old, reset_policy:&policy, weighted_checkpoint: None}).await, ForkOutcome::Failed(_)));
        assert_eq!(unified_persistence::get_session(old).unwrap().unwrap().status, "idle");
        let child = unified_persistence::get_session("osagent-failed-seed-v2").unwrap().unwrap();
        assert_eq!(child.status, "archived");
        let count:i64 = conn.query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id=?1", [&child.session_id], |r|r.get(0)).unwrap();
        assert_eq!(count,0);
        let restart = crate::integrations::gateway::binding::BindingStore::new();
        restart.load_from_db().await.unwrap();
        assert_eq!(restart.get(&key).await.unwrap().target_session_id, old);
    }

    /// Pin the identity-quartet inheritance contract from the docstring on
    /// `build_forked_record`. The four fields below are the same matrix
    /// audited under "P1 Multi-field session identity resolvers" in
    /// `docs/rust-backend/agent-core-cleanup-todo--0429.md`:
    /// dropping any one of them on fork was the original split-brain
    /// shape (a hosted_key parent compacting into an own_key child, etc.).
    #[test]
    fn compaction_fork_preserves_dynamic_owner_without_an_account() {
        let parent = unified_persistence::UnifiedSessionRecord {
            session_id: "package-session".into(),
            model: Some("selected-model".into()),
            credential_source: Some("market:public-selection".into()),
            ..Default::default()
        };
        let child = build_forked_record(
            &parent,
            "package-session-v2",
            "package-session",
            "2026-09-17T00:00:00Z",
        );
        assert_eq!(child.credential_source, parent.credential_source);
        assert_eq!(child.account_id, None);
        assert_eq!(child.model, parent.model);
    }

    #[test]
    fn build_forked_record_inherits_identity_quartet() {
        let parent = unified_persistence::UnifiedSessionRecord {
            session_id: "osagent-telegram-42".into(),
            name: "Channel: telegram".into(),
            status: SessionStatus::Running.as_str().to_string(),
            model: Some("claude-opus-4-6".into()),
            account_id: Some("acct_market_42".into()),
            workspace_path: Some("/repos/example".into()),
            session_type: unified_persistence::session_type::CODING.to_string(),
            channel: Some("telegram".into()),
            chat_id: Some("42".into()),
            agent_definition_id: Some("builtin:sde".into()),
            key_source: KeySource::HostedKey,
            created_at: "2026-04-29T00:00:00Z".into(),
            updated_at: "2026-04-29T00:00:00Z".into(),
            ..Default::default()
        };

        let child = build_forked_record(
            &parent,
            "osagent-telegram-42-v2",
            "osagent-telegram-42",
            "2026-04-29T01:00:00Z",
        );

        assert_eq!(child.session_id, "osagent-telegram-42-v2");
        assert_eq!(
            child.parent_session_id.as_deref(),
            Some("osagent-telegram-42")
        );
        assert_eq!(child.status, SessionStatus::Idle.as_str());
        assert_eq!(child.model, parent.model);
        assert_eq!(child.account_id, parent.account_id);
        assert_eq!(child.workspace_path, parent.workspace_path);
        assert_eq!(
            child.key_source, parent.key_source,
            "key_source must inherit — fork must not silently downgrade \
             a hosted_key parent to an own_key child (billing footgun)"
        );
        assert_eq!(child.session_type, parent.session_type);
        assert_eq!(child.channel, parent.channel);
        assert_eq!(child.chat_id, parent.chat_id);
        assert_eq!(child.agent_definition_id, parent.agent_definition_id);
        assert!(
            child.user_input.is_none(),
            "fork starts with empty user_input"
        );
    }

    #[test]
    fn build_forked_record_inherits_own_key_too() {
        let parent = unified_persistence::UnifiedSessionRecord {
            session_id: "osagent-x".into(),
            account_id: Some("acct_byok".into()),
            key_source: KeySource::OwnKey,
            ..Default::default()
        };
        let child = build_forked_record(&parent, "osagent-x-v2", "osagent-x", "now");
        assert_eq!(child.key_source, KeySource::OwnKey);
        assert_eq!(child.account_id.as_deref(), Some("acct_byok"));
    }

    #[test]
    fn strip_version_suffix_no_suffix() {
        assert_eq!(
            strip_version_suffix("osagent-telegram-42"),
            "osagent-telegram-42"
        );
    }

    #[test]
    fn strip_version_suffix_v2() {
        assert_eq!(
            strip_version_suffix("osagent-telegram-42-v2"),
            "osagent-telegram-42"
        );
    }

    #[test]
    fn strip_version_suffix_high_version() {
        assert_eq!(
            strip_version_suffix("osagent-telegram-42-v123"),
            "osagent-telegram-42"
        );
    }

    #[test]
    fn strip_version_suffix_not_a_version() {
        // "-v" followed by non-digits is NOT a version suffix.
        assert_eq!(strip_version_suffix("osagent-vendor"), "osagent-vendor");
        assert_eq!(strip_version_suffix("osagent-foo-v"), "osagent-foo-v");
        assert_eq!(
            strip_version_suffix("osagent-foo-vbeta"),
            "osagent-foo-vbeta"
        );
    }

    #[test]
    fn strip_version_suffix_idempotent() {
        let once = strip_version_suffix("osagent-telegram-42-v3");
        let twice = strip_version_suffix(once);
        assert_eq!(once, twice);
    }
}
