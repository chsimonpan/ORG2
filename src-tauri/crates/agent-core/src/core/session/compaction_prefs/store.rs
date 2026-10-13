//! Per-session compaction preferences: spend-threshold override and the opt-in
//! idle-compaction switch. One row per session, SQLite is the single source of
//! truth (runtime reads it at each turn start; the UI reads it through RPC).
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const THRESHOLD_MIN: u64 = 10_000;
pub const THRESHOLD_MAX: u64 = 1_000_000_000_000;
pub const IDLE_MINUTES_MIN: u32 = 1;
pub const IDLE_MINUTES_MAX: u32 = 1440;
pub const IDLE_MINUTES_DEFAULT: u32 = 59;

#[derive(Debug)]
pub enum PrefsError {
    Invalid(String),
    Db(rusqlite::Error),
}

impl std::fmt::Display for PrefsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(msg) => write!(f, "{msg}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PrefsError {}

impl From<rusqlite::Error> for PrefsError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(err)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleAttempt {
    pub epoch_ms: i64,
    pub at_ms: i64,
    pub status: String,
    pub tokens_before: Option<i64>,
    pub tokens_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPrefs {
    pub session_id: String,
    /// `None` inherits the agent definition; `Some(0)` disables only the spend trigger.
    pub weighted_threshold: Option<u64>,
    pub idle_enabled: bool,
    pub idle_minutes: u32,
    /// Wall-clock ms of the last turn start/end observed while the switch was on.
    pub last_activity_ms: Option<i64>,
    pub last_attempt: Option<IdleAttempt>,
}

impl CompactionPrefs {
    pub fn defaults(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            weighted_threshold: None,
            idle_enabled: false,
            idle_minutes: IDLE_MINUTES_DEFAULT,
            last_activity_ms: None,
            last_attempt: None,
        }
    }
}

pub fn validate_threshold(value: Option<u64>) -> Result<Option<u64>, PrefsError> {
    match value {
        None | Some(0) => Ok(value),
        Some(v) if (THRESHOLD_MIN..=THRESHOLD_MAX).contains(&v) => Ok(value),
        Some(v) => Err(PrefsError::Invalid(format!(
            "weighted threshold {v} must be 0 (disabled) or between {THRESHOLD_MIN} and {THRESHOLD_MAX}"
        ))),
    }
}

pub fn validate_idle_minutes(minutes: u32) -> Result<u32, PrefsError> {
    if (IDLE_MINUTES_MIN..=IDLE_MINUTES_MAX).contains(&minutes) {
        Ok(minutes)
    } else {
        Err(PrefsError::Invalid(format!(
            "idle minutes {minutes} must be between {IDLE_MINUTES_MIN} and {IDLE_MINUTES_MAX}"
        )))
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_compaction_prefs (
            session_id TEXT PRIMARY KEY,
            weighted_threshold INTEGER,
            idle_enabled INTEGER NOT NULL DEFAULT 0,
            idle_minutes INTEGER NOT NULL DEFAULT 59,
            last_activity_ms INTEGER,
            last_attempt_epoch_ms INTEGER,
            last_attempt_at_ms INTEGER,
            last_attempt_status TEXT,
            last_attempt_tokens_before INTEGER,
            last_attempt_tokens_after INTEGER,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_session_compaction_prefs_idle
            ON session_compaction_prefs(idle_enabled);",
    )
}

const SELECT: &str = "SELECT session_id, weighted_threshold, idle_enabled, idle_minutes,
    last_activity_ms, last_attempt_epoch_ms, last_attempt_at_ms, last_attempt_status,
    last_attempt_tokens_before, last_attempt_tokens_after FROM session_compaction_prefs";

fn row_to_prefs(row: &rusqlite::Row<'_>) -> rusqlite::Result<CompactionPrefs> {
    let attempt = match (
        row.get::<_, Option<i64>>(5)?,
        row.get::<_, Option<String>>(7)?,
    ) {
        (Some(epoch_ms), Some(status)) => Some(IdleAttempt {
            epoch_ms,
            at_ms: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            status,
            tokens_before: row.get(8)?,
            tokens_after: row.get(9)?,
        }),
        _ => None,
    };
    Ok(CompactionPrefs {
        session_id: row.get(0)?,
        weighted_threshold: row.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
        idle_enabled: row.get::<_, i64>(2)? != 0,
        idle_minutes: row
            .get::<_, i64>(3)?
            .clamp(IDLE_MINUTES_MIN as i64, IDLE_MINUTES_MAX as i64) as u32,
        last_activity_ms: row.get(4)?,
        last_attempt: attempt,
    })
}

/// Missing row = defaults. Never creates a row.
pub fn get_with_connection(conn: &Connection, session_id: &str) -> rusqlite::Result<CompactionPrefs> {
    init_schema(conn)?;
    let found = conn
        .query_row(
            &format!("{SELECT} WHERE session_id=?1"),
            [session_id],
            row_to_prefs,
        )
        .optional()?;
    Ok(found.unwrap_or_else(|| CompactionPrefs::defaults(session_id)))
}

pub fn list_idle_enabled_with_connection(conn: &Connection) -> rusqlite::Result<Vec<CompactionPrefs>> {
    init_schema(conn)?;
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE idle_enabled=1"))?;
    let rows = stmt.query_map([], row_to_prefs)?;
    rows.collect()
}

fn ensure_row(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    init_schema(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO session_compaction_prefs(session_id, updated_at_ms) VALUES (?1, ?2)",
        params![session_id, now_ms()],
    )?;
    Ok(())
}

pub fn set_threshold_with_connection(
    conn: &Connection,
    session_id: &str,
    value: Option<u64>,
) -> Result<CompactionPrefs, PrefsError> {
    let value = validate_threshold(value)?;
    ensure_row(conn, session_id)?;
    conn.execute(
        "UPDATE session_compaction_prefs SET weighted_threshold=?2, updated_at_ms=?3 WHERE session_id=?1",
        params![session_id, value.map(|v| v as i64), now_ms()],
    )?;
    Ok(get_with_connection(conn, session_id)?)
}

/// Enabling does not stamp activity: the inactivity clock starts at the next
/// backend turn start/end, never at the toggle (a session that has been idle
/// for hours would otherwise be compacted against an already-expired cache).
pub fn set_idle_with_connection(
    conn: &Connection,
    session_id: &str,
    enabled: bool,
    minutes: u32,
) -> Result<CompactionPrefs, PrefsError> {
    let minutes = validate_idle_minutes(minutes)?;
    ensure_row(conn, session_id)?;
    conn.execute(
        "UPDATE session_compaction_prefs SET idle_enabled=?2, idle_minutes=?3, updated_at_ms=?4 WHERE session_id=?1",
        params![session_id, enabled as i64, minutes as i64, now_ms()],
    )?;
    Ok(get_with_connection(conn, session_id)?)
}

/// Only updates an existing row whose switch is on; never creates rows, so
/// sessions that never opted in cost one failed UPDATE and nothing else.
pub fn touch_activity_with_connection(
    conn: &Connection,
    session_id: &str,
    wall_ms: i64,
) -> rusqlite::Result<bool> {
    init_schema(conn)?;
    let changed = conn.execute(
        "UPDATE session_compaction_prefs SET last_activity_ms=?2 WHERE session_id=?1 AND idle_enabled=1",
        params![session_id, wall_ms],
    )?;
    Ok(changed > 0)
}

pub fn record_attempt_with_connection(
    conn: &Connection,
    session_id: &str,
    attempt: &IdleAttempt,
) -> rusqlite::Result<()> {
    init_schema(conn)?;
    conn.execute(
        "UPDATE session_compaction_prefs SET last_attempt_epoch_ms=?2, last_attempt_at_ms=?3,
         last_attempt_status=?4, last_attempt_tokens_before=?5, last_attempt_tokens_after=?6
         WHERE session_id=?1",
        params![
            session_id,
            attempt.epoch_ms,
            attempt.at_ms,
            attempt.status,
            attempt.tokens_before,
            attempt.tokens_after
        ],
    )?;
    Ok(())
}

pub fn delete_with_connection(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    init_schema(conn)?;
    conn.execute(
        "DELETE FROM session_compaction_prefs WHERE session_id=?1",
        [session_id],
    )?;
    Ok(())
}

fn with_reader<T>(f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let conn = database::db::get_connection()?;
    f(&conn)
}

fn with_writer<T, E: From<rusqlite::Error>>(
    f: impl FnOnce(&Connection) -> Result<T, E>,
) -> Result<T, E> {
    database::db::with_sessions_writer(|| {
        let conn = database::db::get_connection()?;
        f(&conn)
    })
}

pub fn get(session_id: &str) -> rusqlite::Result<CompactionPrefs> {
    with_reader(|conn| get_with_connection(conn, session_id))
}

pub fn list_idle_enabled() -> rusqlite::Result<Vec<CompactionPrefs>> {
    with_reader(list_idle_enabled_with_connection)
}

pub fn set_threshold(session_id: &str, value: Option<u64>) -> Result<CompactionPrefs, PrefsError> {
    validate_threshold(value)?;
    with_writer(|conn| set_threshold_with_connection(conn, session_id, value))
}

pub fn set_idle(
    session_id: &str,
    enabled: bool,
    minutes: u32,
) -> Result<CompactionPrefs, PrefsError> {
    validate_idle_minutes(minutes)?;
    with_writer(|conn| set_idle_with_connection(conn, session_id, enabled, minutes))
}

pub fn touch_activity(session_id: &str, wall_ms: i64) -> rusqlite::Result<bool> {
    with_writer(|conn| touch_activity_with_connection(conn, session_id, wall_ms))
}

pub fn record_attempt(session_id: &str, attempt: &IdleAttempt) -> rusqlite::Result<()> {
    with_writer(|conn| record_attempt_with_connection(conn, session_id, attempt))
}

pub fn delete(session_id: &str) -> rusqlite::Result<()> {
    with_writer(|conn| delete_with_connection(conn, session_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn missing_row_is_defaults_and_not_created() {
        let conn = mem();
        let prefs = get_with_connection(&conn, "s1").unwrap();
        assert_eq!(prefs, CompactionPrefs::defaults("s1"));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_compaction_prefs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn threshold_validation_and_round_trip() {
        let conn = mem();
        for bad in [1u64, 9_999, THRESHOLD_MAX + 1] {
            assert!(matches!(
                set_threshold_with_connection(&conn, "s1", Some(bad)),
                Err(PrefsError::Invalid(_))
            ));
        }
        assert_eq!(
            set_threshold_with_connection(&conn, "s1", Some(5_000_000))
                .unwrap()
                .weighted_threshold,
            Some(5_000_000)
        );
        assert_eq!(
            set_threshold_with_connection(&conn, "s1", Some(0))
                .unwrap()
                .weighted_threshold,
            Some(0)
        );
        assert_eq!(
            set_threshold_with_connection(&conn, "s1", None)
                .unwrap()
                .weighted_threshold,
            None
        );
    }

    #[test]
    fn idle_validation_and_enable_does_not_stamp_activity() {
        let conn = mem();
        for bad in [0u32, 1441] {
            assert!(matches!(
                set_idle_with_connection(&conn, "s1", true, bad),
                Err(PrefsError::Invalid(_))
            ));
        }
        let prefs = set_idle_with_connection(&conn, "s1", true, 59).unwrap();
        assert!(prefs.idle_enabled);
        assert_eq!(prefs.idle_minutes, 59);
        assert_eq!(prefs.last_activity_ms, None);
    }

    #[test]
    fn touch_activity_only_affects_enabled_rows_and_creates_none() {
        let conn = mem();
        assert!(!touch_activity_with_connection(&conn, "never", 10).unwrap());
        set_threshold_with_connection(&conn, "off", Some(20_000)).unwrap();
        assert!(!touch_activity_with_connection(&conn, "off", 10).unwrap());
        set_idle_with_connection(&conn, "on", true, 5).unwrap();
        assert!(touch_activity_with_connection(&conn, "on", 1234).unwrap());
        assert_eq!(
            get_with_connection(&conn, "on").unwrap().last_activity_ms,
            Some(1234)
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_compaction_prefs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn attempt_round_trip_and_enabled_listing() {
        let conn = mem();
        set_idle_with_connection(&conn, "a", true, 10).unwrap();
        set_idle_with_connection(&conn, "b", false, 10).unwrap();
        let attempt = IdleAttempt {
            epoch_ms: 100,
            at_ms: 200,
            status: "compacted".into(),
            tokens_before: Some(9000),
            tokens_after: Some(1000),
        };
        record_attempt_with_connection(&conn, "a", &attempt).unwrap();
        let listed = list_idle_enabled_with_connection(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, "a");
        assert_eq!(listed[0].last_attempt.as_ref(), Some(&attempt));
        assert_eq!(get_with_connection(&conn, "a").unwrap().last_attempt, Some(attempt));
    }

    #[test]
    fn delete_removes_row_and_is_idempotent() {
        let conn = mem();
        set_idle_with_connection(&conn, "a", true, 10).unwrap();
        delete_with_connection(&conn, "a").unwrap();
        delete_with_connection(&conn, "a").unwrap();
        assert_eq!(get_with_connection(&conn, "a").unwrap(), CompactionPrefs::defaults("a"));
    }
}
