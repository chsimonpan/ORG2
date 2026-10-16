//! Pure idle-compaction decision function. No clock, no I/O: the monitor feeds
//! wall-clock `now_ms` so suspend/resume and restarts are handled by comparing
//! wall time, and tests can exercise the whole matrix deterministically.

/// A wake later than `due + GRACE` missed its idle slot (suspend, app was
/// closed). The missed attempt is not dropped and not run at wake time: it is
/// merged into the next idle due slot (`due + k * interval`, first slot after
/// now), which fires once like an ordinary due attempt.
pub const OVERDUE_GRACE_MS: i64 = 2 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanInput {
    pub enabled: bool,
    pub idle_minutes: u32,
    pub last_activity_ms: Option<i64>,
    pub last_attempt_epoch_ms: Option<i64>,
    /// Scheduler has an active job or queued work for the session.
    pub busy: bool,
    /// Merged slot chosen when this epoch's due time was missed (D1). Owned
    /// by the monitor; `None` until a miss is observed.
    pub deferred_due_ms: Option<i64>,
    pub now_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Disabled,
    /// Enabled but no backend turn activity has been seen yet.
    WaitForActivity,
    /// This activity epoch already had its single attempt.
    AlreadyAttempted,
    /// Work in flight; the turn end stamps new activity and replans.
    Busy,
    Wait { due_ms: i64 },
    Fire { epoch_ms: i64 },
    /// Due time was missed (suspend/closed app): wait for the next idle slot
    /// instead of firing now or dropping the attempt. The caller remembers
    /// `due_ms` as `deferred_due_ms` for this epoch.
    Defer { due_ms: i64 },
}

pub fn due_ms(last_activity_ms: i64, idle_minutes: u32) -> i64 {
    last_activity_ms.saturating_add((idle_minutes as i64).saturating_mul(60_000))
}

pub fn plan(input: PlanInput) -> Decision {
    if !input.enabled {
        return Decision::Disabled;
    }
    let Some(epoch_ms) = input.last_activity_ms else {
        return Decision::WaitForActivity;
    };
    if input.last_attempt_epoch_ms == Some(epoch_ms) {
        return Decision::AlreadyAttempted;
    }
    if input.busy {
        return Decision::Busy;
    }
    let due = due_ms(epoch_ms, input.idle_minutes);
    if input.now_ms < due {
        return Decision::Wait { due_ms: due };
    }
    if input.now_ms <= due.saturating_add(OVERDUE_GRACE_MS) {
        return Decision::Fire { epoch_ms };
    }
    // Missed slot. A previously chosen merged slot fires inside its own grace
    // window; otherwise (first miss, or the merged slot was missed too) pick
    // the next slot strictly after now. All missed slots collapse into one.
    if let Some(deferred) = input.deferred_due_ms.filter(|d| *d > due) {
        if input.now_ms < deferred {
            return Decision::Wait { due_ms: deferred };
        }
        if input.now_ms <= deferred.saturating_add(OVERDUE_GRACE_MS) {
            return Decision::Fire { epoch_ms };
        }
    }
    let interval = (input.idle_minutes.max(1) as i64).saturating_mul(60_000);
    let missed = (input.now_ms.saturating_sub(due) / interval).saturating_add(1);
    Decision::Defer {
        due_ms: due.saturating_add(missed.saturating_mul(interval)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn base() -> PlanInput {
        PlanInput {
            enabled: true,
            idle_minutes: 59,
            last_activity_ms: Some(1_000_000),
            last_attempt_epoch_ms: None,
            busy: false,
            deferred_due_ms: None,
            now_ms: 1_000_000,
        }
    }

    #[test]
    fn disabled_wins_over_everything() {
        let input = PlanInput {
            enabled: false,
            busy: true,
            ..base()
        };
        assert_eq!(plan(input), Decision::Disabled);
    }

    #[test]
    fn no_activity_waits_for_activity_not_for_a_clock() {
        let input = PlanInput {
            last_activity_ms: None,
            ..base()
        };
        assert_eq!(plan(input), Decision::WaitForActivity);
    }

    #[test]
    fn before_due_waits_until_exact_due() {
        let input = PlanInput {
            now_ms: 1_000_000 + 58 * MIN,
            ..base()
        };
        assert_eq!(
            plan(input),
            Decision::Wait {
                due_ms: 1_000_000 + 59 * MIN
            }
        );
    }

    #[test]
    fn due_boundary_and_grace_window_fire_once() {
        let at_due = PlanInput {
            now_ms: 1_000_000 + 59 * MIN,
            ..base()
        };
        assert_eq!(plan(at_due), Decision::Fire { epoch_ms: 1_000_000 });
        let at_grace = PlanInput {
            now_ms: 1_000_000 + 59 * MIN + OVERDUE_GRACE_MS,
            ..base()
        };
        assert_eq!(plan(at_grace), Decision::Fire { epoch_ms: 1_000_000 });
    }

    #[test]
    fn wake_after_suspend_beyond_grace_defers_to_next_idle_slot() {
        // D1 (user decision 2026-10-16): a missed slot is neither skipped nor
        // run at wake time; it waits for the next idle due slot.
        let input = PlanInput {
            now_ms: 1_000_000 + 59 * MIN + OVERDUE_GRACE_MS + 1,
            ..base()
        };
        assert_eq!(
            plan(input),
            Decision::Defer {
                due_ms: 1_000_000 + 2 * 59 * MIN
            }
        );
        let deferred = Some(1_000_000 + 2 * 59 * MIN);
        let before_slot = PlanInput {
            deferred_due_ms: deferred,
            now_ms: 1_000_000 + 2 * 59 * MIN - 1,
            ..base()
        };
        assert_eq!(
            plan(before_slot),
            Decision::Wait {
                due_ms: 1_000_000 + 2 * 59 * MIN
            }
        );
        let at_next_slot = PlanInput {
            deferred_due_ms: deferred,
            now_ms: 1_000_000 + 2 * 59 * MIN,
            ..base()
        };
        assert_eq!(plan(at_next_slot), Decision::Fire { epoch_ms: 1_000_000 });
    }

    #[test]
    fn many_missed_slots_merge_into_one_next_slot() {
        let input = PlanInput {
            now_ms: 1_000_000 + 59 * MIN * 10 + 5 * MIN,
            ..base()
        };
        assert_eq!(
            plan(input),
            Decision::Defer {
                due_ms: 1_000_000 + 59 * MIN * 11
            }
        );
        // Missing the merged slot too re-merges into the following one.
        let missed_again = PlanInput {
            deferred_due_ms: Some(1_000_000 + 59 * MIN * 11),
            now_ms: 1_000_000 + 59 * MIN * 11 + OVERDUE_GRACE_MS + 1,
            ..base()
        };
        assert_eq!(
            plan(missed_again),
            Decision::Defer {
                due_ms: 1_000_000 + 59 * MIN * 12
            }
        );
        // After the merged attempt is recorded, the epoch never refires.
        let after = PlanInput {
            last_attempt_epoch_ms: Some(1_000_000),
            now_ms: 1_000_000 + 59 * MIN * 30,
            ..base()
        };
        assert_eq!(plan(after), Decision::AlreadyAttempted);
    }

    #[test]
    fn same_epoch_never_refires_even_when_due_again() {
        let input = PlanInput {
            last_attempt_epoch_ms: Some(1_000_000),
            now_ms: 1_000_000 + 600 * MIN,
            ..base()
        };
        assert_eq!(plan(input), Decision::AlreadyAttempted);
    }

    #[test]
    fn new_activity_starts_a_new_epoch_after_an_attempt() {
        let input = PlanInput {
            last_activity_ms: Some(5_000_000),
            last_attempt_epoch_ms: Some(1_000_000),
            now_ms: 5_000_000,
            ..base()
        };
        assert_eq!(
            plan(input),
            Decision::Wait {
                due_ms: 5_000_000 + 59 * MIN
            }
        );
    }

    #[test]
    fn busy_defers_even_when_overdue() {
        let input = PlanInput {
            busy: true,
            now_ms: 1_000_000 + 1000 * MIN,
            ..base()
        };
        assert_eq!(plan(input), Decision::Busy);
    }

    #[test]
    fn interval_change_replans_from_the_same_epoch() {
        let shorter = PlanInput {
            idle_minutes: 5,
            now_ms: 1_000_000 + 5 * MIN,
            ..base()
        };
        assert_eq!(plan(shorter), Decision::Fire { epoch_ms: 1_000_000 });
        let longer = PlanInput {
            idle_minutes: 120,
            now_ms: 1_000_000 + 59 * MIN,
            ..base()
        };
        assert_eq!(
            plan(longer),
            Decision::Wait {
                due_ms: 1_000_000 + 120 * MIN
            }
        );
    }

    #[test]
    fn clock_stepping_backwards_waits_without_overflow() {
        let input = PlanInput {
            now_ms: 0,
            ..base()
        };
        assert!(matches!(plan(input), Decision::Wait { .. }));
        let huge = PlanInput {
            last_activity_ms: Some(i64::MAX - 10),
            now_ms: i64::MAX,
            ..base()
        };
        let _ = plan(huge);
    }
}
