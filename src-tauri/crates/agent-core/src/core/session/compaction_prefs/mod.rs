//! Per-session compaction preferences (spend-threshold override + idle
//! compaction opt-in). See `planner` for the pure idle decision function and
//! `store` for persistence.

pub mod idle_monitor;
pub mod planner;
mod store;

pub use store::*;

/// Effective spend threshold for a session: the persisted override when set,
/// otherwise the agent-definition default. `source` feeds the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdSource {
    Session,
    AgentDefault,
}

pub fn resolve_threshold(override_value: Option<u64>, agent_default: u64) -> (u64, ThresholdSource) {
    match override_value {
        Some(value) => (value, ThresholdSource::Session),
        None => (agent_default, ThresholdSource::AgentDefault),
    }
}

/// Pre-turn read. A DB failure falls back to the agent default so a prefs
/// problem can never suppress compaction or block a turn.
pub fn effective_threshold(session_id: &str, agent_default: u64) -> (u64, ThresholdSource) {
    match get(session_id) {
        Ok(prefs) => resolve_threshold(prefs.weighted_threshold, agent_default),
        Err(err) => {
            tracing::warn!(
                session_id,
                error = %err,
                "compaction prefs unreadable; using agent default threshold"
            );
            (agent_default, ThresholdSource::AgentDefault)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_beats_default_and_zero_is_a_real_override() {
        assert_eq!(
            resolve_threshold(None, 5_000_000),
            (5_000_000, ThresholdSource::AgentDefault)
        );
        assert_eq!(
            resolve_threshold(Some(800_000), 5_000_000),
            (800_000, ThresholdSource::Session)
        );
        assert_eq!(
            resolve_threshold(Some(0), 5_000_000),
            (0, ThresholdSource::Session)
        );
    }
}
