//! Opt-in idle compaction monitor.
//!
//! Activity = a *turn* job starting or finishing on a session's scheduler
//! (wall-clock). UI input, focus and maintenance jobs never count, so the
//! monitor cannot be kept alive by compaction itself. Event-driven: one task
//! sleeps until the earliest due time and is woken by `Notify`; with no opted-in
//! session it parks on the notify and costs nothing.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::{Notify, Semaphore};
use tracing::{info, warn};

use super::planner::{self, Decision, PlanInput};
use super::{CompactionPrefs, IdleAttempt};
use crate::state::AgentAppState;

#[derive(Debug, Clone)]
struct Entry {
    idle_minutes: u32,
    last_activity_ms: Option<i64>,
    persisted_activity_ms: Option<i64>,
    last_attempt_epoch_ms: Option<i64>,
    in_flight: bool,
}

pub(crate) struct Shared {
    entries: Mutex<HashMap<String, Entry>>,
    wake: Notify,
    /// App-wide single flight: at most one idle compaction runs at a time.
    permit: Arc<Semaphore>,
}

impl Shared {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            wake: Notify::new(),
            permit: Arc::new(Semaphore::new(1)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn global() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(Shared::new)
}

// ---------------------------------------------------------------------------
// Scheduler / RPC facing API (all cheap, no I/O, no awaiting)
// ---------------------------------------------------------------------------

/// Called by the scheduler worker when a Turn job starts and when it ends.
pub fn note_turn_activity(session_id: &str) {
    note_turn_activity_at(global(), session_id, super::now_ms());
}

/// Called by the scheduler worker after a non-turn job (e.g. compaction) ends:
/// replan without stamping activity.
pub fn wake() {
    global().wake.notify_one();
}

fn note_turn_activity_at(shared: &Shared, session_id: &str, now_ms: i64) {
    let touched = {
        let mut entries = shared.lock();
        match entries.get_mut(session_id) {
            Some(entry) => {
                entry.last_activity_ms = Some(now_ms);
                true
            }
            None => false,
        }
    };
    if touched {
        shared.wake.notify_one();
    }
}

/// True while `epoch_ms` is still the session's latest activity stamp and the
/// switch is on. Used inside the exclusive maintenance job before any model call.
pub fn epoch_still_current(session_id: &str, epoch_ms: i64) -> bool {
    epoch_current(global(), session_id, epoch_ms)
}

fn epoch_current(shared: &Shared, session_id: &str, epoch_ms: i64) -> bool {
    shared
        .lock()
        .get(session_id)
        .is_some_and(|entry| entry.last_activity_ms == Some(epoch_ms))
}

/// Apply persisted prefs to the live registry (switch on/off, interval change).
pub fn configure(prefs: &CompactionPrefs) {
    configure_in(global(), prefs);
}

fn configure_in(shared: &Shared, prefs: &CompactionPrefs) {
    {
        let mut entries = shared.lock();
        if prefs.idle_enabled {
            let attempt_epoch = prefs.last_attempt.as_ref().map(|a| a.epoch_ms);
            entries
                .entry(prefs.session_id.clone())
                .and_modify(|entry| entry.idle_minutes = prefs.idle_minutes)
                .or_insert(Entry {
                    idle_minutes: prefs.idle_minutes,
                    last_activity_ms: prefs.last_activity_ms,
                    persisted_activity_ms: prefs.last_activity_ms,
                    last_attempt_epoch_ms: attempt_epoch,
                    in_flight: false,
                });
        } else {
            entries.remove(&prefs.session_id);
        }
    }
    shared.wake.notify_one();
}

/// Drop a session from the registry (session deleted).
pub fn forget(session_id: &str) {
    global().lock().remove(session_id);
    global().wake.notify_one();
}

/// Fresher-than-DB view for the UI: (last activity, last attempted epoch, in flight).
pub fn live_view(session_id: &str) -> Option<(Option<i64>, Option<i64>, bool)> {
    global().lock().get(session_id).map(|entry| {
        (
            entry.last_activity_ms,
            entry.last_attempt_epoch_ms,
            entry.in_flight,
        )
    })
}

// ---------------------------------------------------------------------------
// Evaluation (pure over the registry; unit-tested with an injected clock)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Evaluation {
    pub fires: Vec<(String, i64)>,
    /// (session, epoch, due) recorded as `skipped_overdue`.
    pub overdue: Vec<(String, i64, i64)>,
    pub next_due_ms: Option<i64>,
}

/// Sessions whose due time has passed and could act now; the actor resolves
/// scheduler busy-ness for exactly these (no per-session work otherwise).
fn due_candidates(shared: &Shared, now_ms: i64) -> Vec<String> {
    shared
        .lock()
        .iter()
        .filter(|(_, entry)| {
            !entry.in_flight
                && entry.last_activity_ms.is_some_and(|activity| {
                    entry.last_attempt_epoch_ms != Some(activity)
                        && now_ms >= planner::due_ms(activity, entry.idle_minutes)
                })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn evaluate(shared: &Shared, now_ms: i64, busy: &HashSet<String>) -> Evaluation {
    let mut out = Evaluation::default();
    let mut entries = shared.lock();
    for (id, entry) in entries.iter_mut() {
        if entry.in_flight {
            continue;
        }
        let decision = planner::plan(PlanInput {
            enabled: true,
            idle_minutes: entry.idle_minutes,
            last_activity_ms: entry.last_activity_ms,
            last_attempt_epoch_ms: entry.last_attempt_epoch_ms,
            busy: busy.contains(id),
            now_ms,
        });
        match decision {
            Decision::Wait { due_ms } => {
                out.next_due_ms = Some(out.next_due_ms.map_or(due_ms, |cur| cur.min(due_ms)));
            }
            Decision::Fire { epoch_ms } => {
                entry.in_flight = true;
                out.fires.push((id.clone(), epoch_ms));
            }
            Decision::SkipOverdue { epoch_ms, due_ms } => {
                entry.last_attempt_epoch_ms = Some(epoch_ms);
                out.overdue.push((id.clone(), epoch_ms, due_ms));
            }
            Decision::Disabled
            | Decision::WaitForActivity
            | Decision::AlreadyAttempted
            | Decision::Busy => {}
        }
    }
    out.fires.sort();
    out.overdue.sort();
    out
}

fn dirty_activity(shared: &Shared) -> Vec<(String, i64)> {
    let mut entries = shared.lock();
    entries
        .iter_mut()
        .filter_map(|(id, entry)| match entry.last_activity_ms {
            Some(ms) if entry.persisted_activity_ms != Some(ms) => {
                entry.persisted_activity_ms = Some(ms);
                Some((id.clone(), ms))
            }
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

pub fn spawn(state: AgentAppState) {
    match super::list_idle_enabled() {
        Ok(rows) => {
            for prefs in &rows {
                configure(prefs);
            }
            info!(sessions = rows.len(), "[IdleCompaction] monitor loaded");
        }
        Err(err) => warn!(error = %err, "[IdleCompaction] could not load opted-in sessions"),
    }
    tauri::async_runtime::spawn(run(state));
}

async fn run(state: AgentAppState) {
    let shared = global();
    loop {
        // Arm the wakeup before evaluating so a notify during evaluation is not lost.
        let notified = shared.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        persist_dirty(shared).await;

        let now_ms = super::now_ms();
        let mut busy = HashSet::new();
        for id in due_candidates(shared, now_ms) {
            if session_busy(&state, &id).await {
                busy.insert(id);
            }
        }
        let evaluation = evaluate(shared, now_ms, &busy);

        for (id, epoch_ms, due_ms) in evaluation.overdue {
            info!(
                session_id = %id,
                overdue_ms = now_ms - due_ms,
                "[IdleCompaction] skipped: due time long past (suspend/restart)"
            );
            record_attempt(
                &id,
                IdleAttempt {
                    epoch_ms,
                    at_ms: now_ms,
                    status: "skipped_overdue".into(),
                    tokens_before: None,
                    tokens_after: None,
                },
            )
            .await;
        }
        for (id, epoch_ms) in evaluation.fires {
            tauri::async_runtime::spawn(fire(state.clone(), id, epoch_ms));
        }

        match evaluation.next_due_ms {
            Some(due_ms) => {
                let wait = Duration::from_millis((due_ms - super::now_ms()).max(0) as u64);
                tokio::select! {
                    _ = &mut notified => {}
                    _ = tokio::time::sleep(wait) => {}
                }
            }
            None => notified.await,
        }
    }
}

async fn session_busy(state: &AgentAppState, session_id: &str) -> bool {
    match state.get_session(session_id).await {
        Some(session) => session.scheduler.is_processing() || session.scheduler.pending_count() > 0,
        None => false,
    }
}

async fn persist_dirty(shared: &Shared) {
    let dirty = dirty_activity(shared);
    if dirty.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        for (id, ms) in dirty {
            if let Err(err) = super::touch_activity(&id, ms) {
                warn!(session_id = %id, error = %err, "[IdleCompaction] activity persist failed");
            }
        }
    })
    .await;
}

async fn record_attempt(session_id: &str, attempt: IdleAttempt) {
    let id = session_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(err) = super::record_attempt(&id, &attempt) {
            warn!(session_id = %id, error = %err, "[IdleCompaction] attempt persist failed");
        }
    })
    .await;
    crate::foundation::bus::broadcast_event(
        "agent:compaction_prefs_changed",
        serde_json::json!({ "sessionId": session_id }),
    );
}

async fn fire(state: AgentAppState, session_id: String, epoch_ms: i64) {
    let shared = global();
    let permit = Arc::clone(&shared.permit).acquire_owned().await;
    let outcome = if permit.is_err() || !epoch_current(shared, &session_id, epoch_ms) {
        // Activity resumed while waiting for the single-flight permit; the
        // new epoch plans itself. Not an attempt.
        None
    } else {
        info!(session_id = %session_id, "[IdleCompaction] firing");
        Some(
            crate::state::commands::session::run_scheduled_compaction(
                &state,
                &session_id,
                None,
                crate::state::commands::session::CompactionTrigger::Idle { epoch_ms },
            )
            .await,
        )
    };
    drop(permit);

    let attempt = match outcome {
        None => None,
        Some(Ok(result)) => {
            use crate::state::commands::session::ManualCompactStatus as S;
            if matches!(result.status, S::Skipped) {
                None
            } else {
                Some(IdleAttempt {
                    epoch_ms,
                    at_ms: super::now_ms(),
                    status: result.status.as_str().to_string(),
                    tokens_before: result.tokens_before.map(|v| v as i64),
                    tokens_after: result.tokens_after.map(|v| v as i64),
                })
            }
        }
        Some(Err(err)) => {
            warn!(session_id = %session_id, error = %err, "[IdleCompaction] compaction error");
            Some(IdleAttempt {
                epoch_ms,
                at_ms: super::now_ms(),
                status: "failed".into(),
                tokens_before: None,
                tokens_after: None,
            })
        }
    };

    {
        let mut entries = shared.lock();
        if let Some(entry) = entries.get_mut(&session_id) {
            entry.in_flight = false;
            if attempt.is_some() {
                entry.last_attempt_epoch_ms = Some(epoch_ms);
            }
        }
    }
    if let Some(attempt) = attempt {
        info!(session_id = %session_id, status = %attempt.status, "[IdleCompaction] attempt finished");
        record_attempt(&session_id, attempt).await;
    }
    shared.wake.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn prefs(id: &str, minutes: u32, activity: Option<i64>) -> CompactionPrefs {
        CompactionPrefs {
            idle_enabled: true,
            idle_minutes: minutes,
            last_activity_ms: activity,
            ..CompactionPrefs::defaults(id)
        }
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn only_opted_in_sessions_are_tracked_and_stamped() {
        let shared = Shared::new();
        note_turn_activity_at(&shared, "stranger", 5);
        assert!(shared.lock().is_empty());
        configure_in(&shared, &prefs("a", 59, None));
        note_turn_activity_at(&shared, "a", 7);
        assert_eq!(shared.lock()["a"].last_activity_ms, Some(7));
        configure_in(&shared, &CompactionPrefs::defaults("a"));
        assert!(shared.lock().is_empty(), "switch off evicts the entry");
    }

    #[test]
    fn reconfigure_keeps_fresher_in_memory_activity_and_updates_interval() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(100)));
        note_turn_activity_at(&shared, "a", 900);
        configure_in(&shared, &prefs("a", 10, Some(100)));
        let entries = shared.lock();
        assert_eq!(entries["a"].last_activity_ms, Some(900));
        assert_eq!(entries["a"].idle_minutes, 10);
    }

    #[test]
    fn fires_once_per_epoch_and_marks_in_flight() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(1_000)));
        let due = 1_000 + 59 * MIN;
        let first = evaluate(&shared, due, &none());
        assert_eq!(first.fires, vec![("a".to_string(), 1_000)]);
        let second = evaluate(&shared, due + 1, &none());
        assert!(second.fires.is_empty(), "in-flight entry is not re-fired");
        shared.lock().get_mut("a").unwrap().in_flight = false;
        shared.lock().get_mut("a").unwrap().last_attempt_epoch_ms = Some(1_000);
        let third = evaluate(&shared, due + 2, &none());
        assert!(third.fires.is_empty(), "attempted epoch is final");
    }

    #[test]
    fn waits_report_the_earliest_due_across_sessions() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(0)));
        configure_in(&shared, &prefs("b", 10, Some(0)));
        let ev = evaluate(&shared, MIN, &none());
        assert_eq!(ev.next_due_ms, Some(10 * MIN));
        assert!(ev.fires.is_empty());
    }

    #[test]
    fn busy_session_is_deferred_and_a_new_turn_stamp_starts_a_new_epoch() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 5, Some(0)));
        let busy: HashSet<String> = ["a".to_string()].into();
        let ev = evaluate(&shared, 5 * MIN, &busy);
        assert!(ev.fires.is_empty() && ev.overdue.is_empty());
        note_turn_activity_at(&shared, "a", 6 * MIN);
        let ev = evaluate(&shared, 6 * MIN, &none());
        assert_eq!(ev.next_due_ms, Some(11 * MIN));
    }

    #[test]
    fn overdue_after_restart_is_recorded_once_and_never_fires() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(0)));
        let now = 10_000 * MIN;
        let ev = evaluate(&shared, now, &none());
        assert!(ev.fires.is_empty());
        assert_eq!(ev.overdue, vec![("a".to_string(), 0, 59 * MIN)]);
        let again = evaluate(&shared, now + MIN, &none());
        assert!(again.fires.is_empty() && again.overdue.is_empty());
    }

    #[test]
    fn persisted_attempt_survives_reload_so_restart_does_not_refire() {
        let shared = Shared::new();
        let mut p = prefs("a", 59, Some(0));
        p.last_attempt = Some(IdleAttempt {
            epoch_ms: 0,
            at_ms: 59 * MIN,
            status: "compacted".into(),
            tokens_before: None,
            tokens_after: None,
        });
        configure_in(&shared, &p);
        let ev = evaluate(&shared, 59 * MIN + 1, &none());
        assert_eq!(ev, Evaluation::default());
    }

    #[test]
    fn epoch_guard_detects_new_activity_and_switch_off() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(10)));
        assert!(epoch_current(&shared, "a", 10));
        note_turn_activity_at(&shared, "a", 11);
        assert!(!epoch_current(&shared, "a", 10));
        configure_in(&shared, &CompactionPrefs::defaults("a"));
        assert!(!epoch_current(&shared, "a", 11));
    }

    #[test]
    fn due_candidates_excludes_waiting_attempted_and_in_flight() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("waiting", 59, Some(0)));
        configure_in(&shared, &prefs("due", 1, Some(0)));
        configure_in(&shared, &prefs("noactivity", 1, None));
        let mut attempted = prefs("attempted", 1, Some(0));
        attempted.last_attempt = Some(IdleAttempt {
            epoch_ms: 0,
            at_ms: 0,
            status: "failed".into(),
            tokens_before: None,
            tokens_after: None,
        });
        configure_in(&shared, &attempted);
        assert_eq!(due_candidates(&shared, 2 * MIN), vec!["due".to_string()]);
    }

    #[test]
    fn dirty_activity_is_flushed_once() {
        let shared = Shared::new();
        configure_in(&shared, &prefs("a", 59, Some(1)));
        assert!(dirty_activity(&shared).is_empty());
        note_turn_activity_at(&shared, "a", 50);
        assert_eq!(dirty_activity(&shared), vec![("a".to_string(), 50)]);
        assert!(dirty_activity(&shared).is_empty());
    }
}
