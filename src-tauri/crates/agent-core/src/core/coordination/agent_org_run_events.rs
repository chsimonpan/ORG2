//! Run-scoped frontend invalidation for Agent Org read models.
//!
//! The run view is assembled from several durable stores (run, tasks, inbox,
//! interventions, and session runtime rows). Each owner emits the same small
//! invalidation after a successful mutation. The frontend then coalesces these
//! pushes and refreshes one shared projection per mounted run.

use serde::Serialize;

use super::agent_org_runs::AgentOrgRunStore;

pub const AGENT_ORG_RUN_CHANGED_EVENT: &str = "agent_org:run_changed";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentOrgRunChangedPayload<'a> {
    org_run_id: &'a str,
}

/// Notify connected frontends that a run's composite read model is stale.
pub fn notify_agent_org_run_changed(org_run_id: &str) {
    if crate::bus::frontend_subscriber_count() == 0 {
        return;
    }
    crate::bus::broadcast_event(
        AGENT_ORG_RUN_CHANGED_EVENT,
        AgentOrgRunChangedPayload { org_run_id },
    );
}

/// Resolve a changed session back to its Agent Org run, if any, and notify it.
///
/// Session persistence calls this outside its writer guard, so the parent walk
/// can safely open a read connection without extending a write-lock lifetime.
pub fn notify_agent_org_session_changed(session_id: &str) {
    if crate::bus::frontend_subscriber_count() == 0 {
        return;
    }
    match AgentOrgRunStore::run_id_for_session_with_parent_walk(session_id) {
        Ok(Some(org_run_id)) => notify_agent_org_run_changed(&org_run_id),
        Ok(None) => {}
        Err(error) => tracing::warn!(
            session_id,
            error = %error,
            "[agent_org] failed to resolve changed session to a run"
        ),
    }
}
