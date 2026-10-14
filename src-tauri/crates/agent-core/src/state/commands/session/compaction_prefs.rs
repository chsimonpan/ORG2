//! RPC surface for per-session compaction preferences. The backend is the
//! single source of truth: every command returns the full view so the UI never
//! keeps its own copy of effective values.
use serde::Serialize;

use crate::core::model_context::compaction::CompactionConfig;
use crate::session::compaction_prefs::{
    self as prefs, idle_monitor,
    planner::{self, Decision, PlanInput},
    IdleAttempt, ThresholdSource,
};
use crate::session::persistence as unified_persistence;
use crate::state::AgentAppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleView {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
    pub enabled: bool,
    pub minutes: u32,
    pub minutes_min: u32,
    pub minutes_max: u32,
    /// disabled | unsupported | waiting_activity | waiting | busy | in_flight |
    /// attempted | due | overdue
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_due_ms: Option<i64>,
    pub last_attempt: Option<IdleAttempt>,
    pub now_ms: i64,
    pub overdue_grace_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPrefsView {
    pub session_id: String,
    /// Persisted per-session override; `null` inherits the agent default.
    pub weighted_threshold: Option<u64>,
    /// Current equivalent-token spend since the last compaction boundary.
    pub weighted_used_tokens: u64,
    pub agent_default_threshold: u64,
    pub agent_default_from_runtime: bool,
    pub effective_threshold: u64,
    pub threshold_source: ThresholdSource,
    pub threshold_min: u64,
    pub threshold_max: u64,
    pub idle: IdleView,
}

async fn idle_unsupported_reason(state: &AgentAppState, session_id: &str) -> Option<String> {
    if session_id.starts_with(core_types::session::CLI_SESSION_PREFIX) {
        return Some("Native CLI sessions are compacted by their own runtime".into());
    }
    if state.gateway_bindings.find_by_target(session_id).await.is_some() {
        return Some("Channel-attached sessions compact by forking, not in place".into());
    }
    None
}

async fn build_view(state: &AgentAppState, session_id: &str) -> Result<CompactionPrefsView, String> {
    let sid = session_id.to_string();
    let (stored, milli) = tokio::task::spawn_blocking(move || {
        let stored = prefs::get(&sid).map_err(|err| err.to_string())?;
        let milli = crate::model_context::weighted_usage::peek_milli(&sid)
            .map_err(|err| err.to_string())?;
        Ok::<_, String>((stored, milli))
    })
    .await
    .map_err(|err| format!("compaction prefs task failed: {err}"))??;

    let session = state.get_session(session_id).await;
    let runtime = match &session {
        Some(session) => session.get_runtime().await,
        None => None,
    };
    let (agent_default, from_runtime) = match &runtime {
        Some(runtime) => (runtime.resolved.compaction.weighted_token_threshold, true),
        None => (CompactionConfig::default().weighted_token_threshold, false),
    };
    let (effective, source) = prefs::resolve_threshold(stored.weighted_threshold, agent_default);

    let unsupported = idle_unsupported_reason(state, session_id).await;
    let live = idle_monitor::live_view(session_id);
    let (last_activity, attempt_epoch, in_flight) = match live {
        Some((activity, attempt, in_flight)) => (activity, attempt, in_flight),
        None => (
            stored.last_activity_ms,
            stored.last_attempt.as_ref().map(|a| a.epoch_ms),
            false,
        ),
    };
    let busy = session
        .as_ref()
        .is_some_and(|s| s.scheduler.is_processing() || s.scheduler.pending_count() > 0);
    let now_ms = prefs::now_ms();
    let decision = planner::plan(PlanInput {
        enabled: stored.idle_enabled,
        idle_minutes: stored.idle_minutes,
        last_activity_ms: last_activity,
        last_attempt_epoch_ms: attempt_epoch,
        busy,
        now_ms,
    });
    let (state_tag, next_due_ms) = if unsupported.is_some() && !stored.idle_enabled {
        ("unsupported", None)
    } else if in_flight {
        ("in_flight", None)
    } else {
        match decision {
            Decision::Disabled => ("disabled", None),
            Decision::WaitForActivity => ("waiting_activity", None),
            Decision::AlreadyAttempted => ("attempted", None),
            Decision::Busy => ("busy", None),
            Decision::Wait { due_ms } => ("waiting", Some(due_ms)),
            Decision::Fire { .. } => ("due", None),
            Decision::SkipOverdue { .. } => ("overdue", None),
        }
    };

    Ok(CompactionPrefsView {
        session_id: session_id.to_string(),
        weighted_threshold: stored.weighted_threshold,
        weighted_used_tokens: (milli.max(0) / 1000) as u64,
        agent_default_threshold: agent_default,
        agent_default_from_runtime: from_runtime,
        effective_threshold: effective,
        threshold_source: source,
        threshold_min: prefs::THRESHOLD_MIN,
        threshold_max: prefs::THRESHOLD_MAX,
        idle: IdleView {
            supported: unsupported.is_none(),
            unsupported_reason: unsupported,
            enabled: stored.idle_enabled,
            minutes: stored.idle_minutes,
            minutes_min: prefs::IDLE_MINUTES_MIN,
            minutes_max: prefs::IDLE_MINUTES_MAX,
            state: state_tag,
            last_activity_ms: last_activity,
            next_due_ms,
            last_attempt: stored.last_attempt,
            now_ms,
            overdue_grace_ms: planner::OVERDUE_GRACE_MS,
        },
    })
}

async fn ensure_session_exists(session_id: &str) -> Result<(), String> {
    let sid = session_id.to_string();
    let found = tokio::task::spawn_blocking(move || unified_persistence::get_session(&sid))
        .await
        .map_err(|err| format!("session lookup task failed: {err}"))?
        .map_err(|err| err.to_string())?;
    if found.is_none() {
        return Err(format!("Session {session_id} does not exist"));
    }
    Ok(())
}

fn announce_changed(session_id: &str) {
    crate::foundation::bus::broadcast_event(
        "agent:compaction_prefs_changed",
        serde_json::json!({ "sessionId": session_id }),
    );
}

#[tauri::command]
pub async fn session_compaction_prefs_get(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<CompactionPrefsView, String> {
    build_view(state.inner(), &session_id).await
}

#[tauri::command]
pub async fn session_compaction_prefs_set_threshold(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    threshold: Option<u64>,
) -> Result<CompactionPrefsView, String> {
    ensure_session_exists(&session_id).await?;
    let sid = session_id.clone();
    tokio::task::spawn_blocking(move || prefs::set_threshold(&sid, threshold))
        .await
        .map_err(|err| format!("compaction prefs task failed: {err}"))?
        .map_err(|err| err.to_string())?;
    announce_changed(&session_id);
    build_view(state.inner(), &session_id).await
}

#[tauri::command]
pub async fn session_compaction_prefs_set_idle(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    enabled: bool,
    minutes: u32,
) -> Result<CompactionPrefsView, String> {
    ensure_session_exists(&session_id).await?;
    if enabled {
        if let Some(reason) = idle_unsupported_reason(state.inner(), &session_id).await {
            return Err(reason);
        }
    }
    let sid = session_id.clone();
    let stored = tokio::task::spawn_blocking(move || prefs::set_idle(&sid, enabled, minutes))
        .await
        .map_err(|err| format!("compaction prefs task failed: {err}"))?
        .map_err(|err| err.to_string())?;
    idle_monitor::configure(&stored);
    announce_changed(&session_id);
    build_view(state.inner(), &session_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::gateway::binding::SessionKey;

    fn sandbox_db() -> impl std::ops::Deref<Target = rusqlite::Connection> {
        let conn = database::db::get_connection().expect("sandbox database");
        crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
        conn
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wire_json_always_carries_last_attempt_as_null_when_none() {
        let _sandbox = test_helpers::test_env::sandbox();
        let _conn = sandbox_db();
        let state = AgentAppState::new();
        let view = build_view(&state, "view-wire").await.unwrap();
        assert!(view.idle.last_attempt.is_none());
        let wire = serde_json::to_value(&view).unwrap();
        let idle = wire.get("idle").and_then(|v| v.as_object()).expect("idle object");
        assert_eq!(idle.get("lastAttempt"), Some(&serde_json::Value::Null), "TS contract is `IdleCompactionAttempt | null`; an omitted key fails the strict RPC output schema");
        assert!(wire.get("weightedThreshold").is_some_and(|v| v.is_null()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn default_view_inherits_agent_default_and_reports_spend() {
        let _sandbox = test_helpers::test_env::sandbox();
        let conn = sandbox_db();
        let state = AgentAppState::new();
        let sid = "view-default";
        let view = build_view(&state, sid).await.unwrap();
        assert_eq!(view.weighted_threshold, None);
        assert_eq!(view.threshold_source, ThresholdSource::AgentDefault);
        assert_eq!(view.effective_threshold, CompactionConfig::default().weighted_token_threshold);
        assert!(!view.agent_default_from_runtime);
        assert_eq!(view.weighted_used_tokens, 0);
        assert_eq!(view.idle.state, "disabled");
        assert!(view.idle.supported && !view.idle.enabled);

        crate::model_context::weighted_usage::record_with_connection(&conn, sid, "spend", 0, 1_500_000_000).unwrap();
        prefs::set_threshold(sid, Some(2_000_000)).unwrap();
        let view = build_view(&state, sid).await.unwrap();
        assert_eq!(view.weighted_used_tokens, 1_500_000);
        assert_eq!(view.effective_threshold, 2_000_000);
        assert_eq!(view.threshold_source, ThresholdSource::Session);
        // Reading the view never creates a spend row for a session without one.
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_weighted_usage WHERE session_id='never-seen'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        build_view(&state, "never-seen").await.unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_weighted_usage WHERE session_id='never-seen'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn idle_view_states_follow_the_activity_epoch() {
        let _sandbox = test_helpers::test_env::sandbox();
        let _conn = sandbox_db();
        let state = AgentAppState::new();
        let sid = "view-idle-states";
        let stored = prefs::set_idle(sid, true, 59).unwrap();
        idle_monitor::configure(&stored);
        let view = build_view(&state, sid).await.unwrap();
        assert_eq!(view.idle.state, "waiting_activity", "enabling does not stamp activity");
        assert_eq!(view.idle.next_due_ms, None);

        idle_monitor::note_turn_activity(sid);
        let view = build_view(&state, sid).await.unwrap();
        assert_eq!(view.idle.state, "waiting");
        let due = view.idle.next_due_ms.unwrap();
        assert_eq!(due, view.idle.last_activity_ms.unwrap() + 59 * 60_000);
        assert_eq!(view.idle.overdue_grace_ms, planner::OVERDUE_GRACE_MS);
        idle_monitor::forget(sid);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cli_and_channel_attached_sessions_cannot_opt_in() {
        let _sandbox = test_helpers::test_env::sandbox();
        let _conn = sandbox_db();
        let state = AgentAppState::new();

        let cli = format!("{}view", core_types::session::CLI_SESSION_PREFIX);
        let view = build_view(&state, &cli).await.unwrap();
        assert!(!view.idle.supported);
        assert_eq!(view.idle.state, "unsupported");
        assert!(idle_unsupported_reason(&state, &cli).await.is_some());

        state
            .gateway_bindings
            .set(SessionKey("chan:view".into()), "view-channel".into())
            .await
            .unwrap();
        let view = build_view(&state, "view-channel").await.unwrap();
        assert!(!view.idle.supported);
        assert!(idle_unsupported_reason(&state, "view-plain").await.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn missing_session_is_rejected_before_any_write() {
        let _sandbox = test_helpers::test_env::sandbox();
        let _conn = sandbox_db();
        assert!(ensure_session_exists("no-such-session").await.is_err());
        assert_eq!(prefs::get("no-such-session").unwrap(), prefs::CompactionPrefs::defaults("no-such-session"));
    }
}
