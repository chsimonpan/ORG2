//! Durable equivalent-token accounting. Provider spans already normalize prompt
//! tokens to uncached input (usage_accumulator contract); never subtract cache twice.
//! A receipt is scoped by owner session + turn + iteration, not active UI session.
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeightedCheckpoint {
    pub session_id: String,
    pub generation: i64,
    pub milli: i64,
}

pub fn weighted_milli(input: i64, read: i64, write: i64, output: i64) -> i64 {
    [(input, 1000i64), (read, 100), (write, 1250), (output, 5000)]
        .into_iter()
        .fold(0i64, |sum, (n, weight)| {
            sum.saturating_add(n.max(0).saturating_mul(weight))
        })
}

fn schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_weighted_usage (
        session_id TEXT PRIMARY KEY, generation INTEGER NOT NULL DEFAULT 0,
        milli INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS session_weighted_usage_receipts (
        session_id TEXT NOT NULL, turn_id TEXT NOT NULL, iteration INTEGER NOT NULL,
        PRIMARY KEY(session_id, turn_id, iteration));",
    )
}

pub fn snapshot_with_connection(conn: &Connection, session_id: &str) -> Result<WeightedCheckpoint> {
    schema(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO session_weighted_usage(session_id) VALUES (?1)",
        [session_id],
    )?;
    conn.query_row(
        "SELECT generation, milli FROM session_weighted_usage WHERE session_id=?1",
        [session_id],
        |r| {
            Ok(WeightedCheckpoint {
                session_id: session_id.to_string(),
                generation: r.get(0)?,
                milli: r.get(1)?,
            })
        },
    )
}

/// Caller owns an IMMEDIATE transaction. Receipt + accumulator are one commit.
pub fn record_with_connection(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
    iteration: i64,
    milli: i64,
) -> Result<()> {
    let snapshot = snapshot_with_connection(conn, session_id)?;
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO session_weighted_usage_receipts
        (session_id, turn_id, iteration) VALUES (?1, ?2, ?3)",
        params![session_id, turn_id, iteration],
    )?;
    if inserted > 0 {
        conn.execute(
            "UPDATE session_weighted_usage SET milli=?2 WHERE session_id=?1",
            params![session_id, snapshot.milli.saturating_add(milli.max(0))],
        )?;
    }
    Ok(())
}

/// Called in the SAME transaction as a durable compact boundary insert. Subtract
/// only captured spend; late usage is retained. Generation makes retry idempotent.
pub fn commit_with_connection(conn: &Connection, checkpoint: &WeightedCheckpoint) -> Result<()> {
    schema(conn)?;
    conn.execute(
        "UPDATE session_weighted_usage SET milli=MAX(0,milli-?3), generation=generation+1
        WHERE session_id=?1 AND generation=?2",
        params![
            checkpoint.session_id,
            checkpoint.generation,
            checkpoint.milli
        ],
    )?;
    Ok(())
}

pub fn snapshot(session_id: &str) -> Result<WeightedCheckpoint> {
    database::db::with_sessions_writer(|| {
        let conn = database::db::get_connection()?;
        snapshot_with_connection(&conn, session_id)
    })
}

pub fn record_turn(
    session_id: &str,
    turn_id: &str,
    spans: &[crate::turn_executor::LlmUsageSpan],
) -> Result<()> {
    database::db::with_sessions_writer(|| {
        let mut conn = database::db::get_connection()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for span in spans {
            record_with_connection(
                &tx,
                session_id,
                turn_id,
                span.iteration_index,
                weighted_milli(
                    span.prompt_tokens,
                    span.cache_read_tokens,
                    span.cache_write_tokens,
                    span.completion_tokens,
                ),
            )?;
        }
        tx.commit()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weighted_formula_clamps_and_saturates_without_double_cache() {
        assert_eq!(weighted_milli(10, 20, 30, 40), 249_500);
        assert_eq!(weighted_milli(-1, -1, -1, -1), 0);
        assert_eq!(
            weighted_milli(i64::MAX, i64::MAX, i64::MAX, i64::MAX),
            i64::MAX
        );
    }
    #[test]
    fn weighted_receipts_session_isolation_restart_and_atomic_reset() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tx = conn.transaction().unwrap();
        record_with_connection(&tx, "A", "t1", 1, 4_999_999_000).unwrap();
        record_with_connection(&tx, "A", "t1", 1, 4_999_999_000).unwrap();
        record_with_connection(&tx, "B", "t1", 1, 8_000).unwrap();
        tx.commit().unwrap();
        let checkpoint = snapshot_with_connection(&conn, "A").unwrap();
        assert_eq!(checkpoint.milli, 4_999_999_000);
        // Another request continues while the captured attempt awaits provider.
        record_with_connection(&conn, "A", "t2", 1, 1_000).unwrap();
        {
            let tx = conn.transaction().unwrap();
            commit_with_connection(&tx, &checkpoint).unwrap();
            tx.rollback().unwrap();
        }
        assert_eq!(
            snapshot_with_connection(&conn, "A").unwrap().milli,
            5_000_000_000
        );
        {
            let tx = conn.transaction().unwrap();
            commit_with_connection(&tx, &checkpoint).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(snapshot_with_connection(&conn, "A").unwrap().milli, 1_000);
        commit_with_connection(&conn, &checkpoint).unwrap();
        assert_eq!(snapshot_with_connection(&conn, "A").unwrap().milli, 1_000);
        assert_eq!(snapshot_with_connection(&conn, "B").unwrap().milli, 8_000);
        // Reading afresh is the restart path, no process-local counter involved.
        assert_eq!(snapshot_with_connection(&conn, "A").unwrap().generation, 1);
    }
    #[test]
    fn weighted_restart_reopens_durable_receipts_without_recounting() {
        let dir = tempfile::tempdir_in(std::env::var("ORG2_TEST_TMP").unwrap()).unwrap();
        let path = dir.path().join("weighted.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            record_with_connection(&conn, "A", "turn", 1, 5_000_000_000).unwrap();
        }
        {
            let conn = Connection::open(&path).unwrap();
            record_with_connection(&conn, "A", "turn", 1, 5_000_000_000).unwrap();
            assert_eq!(
                snapshot_with_connection(&conn, "A").unwrap().milli,
                5_000_000_000
            );
        }
    }
    #[test]
    fn weighted_parallel_sessions_share_no_counter_and_duplicate_receipts_are_once() {
        let dir = tempfile::tempdir_in(std::env::var("ORG2_TEST_TMP").unwrap()).unwrap();
        let path = dir.path().join("parallel.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            schema(&conn).unwrap();
        }
        let mut handles = Vec::new();
        for owner in ["A", "B", "A", "B"] {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                let mut conn = Connection::open(path).unwrap();
                conn.busy_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                for i in 0..20 {
                    let tx = conn
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .unwrap();
                    record_with_connection(
                        &tx,
                        owner,
                        "same-turn",
                        i,
                        if owner == "A" { 1000 } else { 2000 },
                    )
                    .unwrap();
                    tx.commit().unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let conn = Connection::open(path).unwrap();
        assert_eq!(snapshot_with_connection(&conn, "A").unwrap().milli, 20_000);
        assert_eq!(snapshot_with_connection(&conn, "B").unwrap().milli, 40_000);
    }
    #[test]
    fn weighted_missing_usage_and_negative_usage_do_not_invent_spend() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(snapshot_with_connection(&conn, "empty").unwrap().milli, 0);
        record_with_connection(&conn, "empty", "turn", 1, weighted_milli(0, 0, 0, 0)).unwrap();
        record_with_connection(&conn, "empty", "turn", 2, weighted_milli(-1, -2, -3, -4)).unwrap();
        assert_eq!(snapshot_with_connection(&conn, "empty").unwrap().milli, 0);
    }
}
