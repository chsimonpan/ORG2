//! Integration tests for the durable review executor: real file-backed SQLite, the real queue,
//! the real Journey aggregate and a scripted provider. No production wiring is faked except the provider.
use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::session::journey_application_service::RetryReviewRequest;
use crate::core::session::journey_lifecycle::{ForkState, JourneyFork, ReviewItem, ReviewState};
use crate::providers::traits::{LLMResponse, ToolCallRequest};

const SECRET_LINE: &str = "frozen transcript line that must never leak into an error";

#[derive(Clone)]
enum Mode {
    Ok,
    Block,
    Fail(String),
    Garbage,
}

struct Shared {
    mode: Mutex<Mode>,
    calls: AtomicUsize,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    entered: Notify,
    delay_ms: u64,
    opens: AtomicUsize,
    script: Mutex<VecDeque<Mode>>,
}

impl Shared {
    fn new(mode: Mode, delay_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            calls: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            entered: Notify::new(),
            delay_ms,
            opens: AtomicUsize::new(0),
            script: Mutex::new(VecDeque::new()),
        })
    }
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

struct Provider {
    shared: Arc<Shared>,
    session: String,
}

#[async_trait]
impl JourneyUnboundedReviewProvider for Provider {
    async fn review_unbounded(&self, _messages: &[Value], _tools: &[Value], _model: &str, _temperature: f32) -> Result<LLMResponse, String> {
        let s = &self.shared;
        s.calls.fetch_add(1, Ordering::SeqCst);
        let now = s.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        s.max_in_flight.fetch_max(now, Ordering::SeqCst);
        s.entered.notify_one();
        struct Leave<'a>(&'a AtomicUsize);
        impl Drop for Leave<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _leave = Leave(&s.in_flight);
        let mode = s.mode.lock().unwrap().clone();
        match mode {
            Mode::Block => std::future::pending::<()>().await,
            Mode::Fail(text) => return Err(text),
            _ => {}
        }
        if s.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(s.delay_ms)).await;
        }
        let args = if matches!(mode, Mode::Garbage) {
            serde_json::json!({"conclusion": "缺字段"})
        } else {
            serde_json::json!({
                "objective": "核对分叉结论", "conclusion": "可以继续主分支", "open_items": ["补测边界"], "confirmed_items": ["证据已覆盖"],
                "evidence_message_ids": [format!("{}-f3", self.session), format!("{}-f5", self.session)],
                "possibly_worthless": false, "critique": ["无额外遗漏"]
            })
        };
        let mut response = LLMResponse::text("");
        response.tool_calls.push(ToolCallRequest { id: "review-call".into(), name: "submit_fork_review".into(), arguments: args, thought_signature: None });
        Ok(response)
    }
}

struct Resolver {
    shared: Arc<Shared>,
    pin: RuntimeProvenance,
    timeout: Duration,
}

#[async_trait]
impl JourneyReviewProviderResolver for Resolver {
    async fn resolve(&self, session_id: &str, expected: &RuntimeProvenance) -> Result<Arc<dyn JourneyUnboundedReviewProvider>, String> {
        if expected != &self.pin {
            return Err("exact provenance mismatch".into());
        }
        if let Some(next) = self.shared.script.lock().unwrap().pop_front() {
            self.shared.set(next);
        }
        Ok(Arc::new(Provider { shared: self.shared.clone(), session: session_id.to_string() }))
    }

    fn stage_timeout(&self) -> Duration {
        self.timeout
    }
}

fn prov() -> RuntimeProvenance {
    RuntimeProvenance { model_id: "anthropic/claude-sonnet-5.5".into(), account_id: "acct-1".into(), protocol: "anthropic".into() }
}

struct Fx {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    shared: Arc<Shared>,
    open: ConnOpener,
    resolver: Arc<Resolver>,
}

struct OwnedConn(Connection);
impl std::ops::Deref for OwnedConn {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedConn {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.0
    }
}

impl Fx {
    fn new(mode: Mode, delay_ms: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.sqlite");
        let shared = Shared::new(mode, delay_ms);
        let (p, sh) = (path.clone(), shared.clone());
        let open: ConnOpener = Arc::new(move || {
            sh.opens.fetch_add(1, Ordering::SeqCst);
            let conn = Connection::open(&p).map_err(|e| QueueError::Storage(e.to_string()))?;
            conn.busy_timeout(Duration::from_secs(5)).map_err(|e| QueueError::Storage(e.to_string()))?;
            Ok(Box::new(OwnedConn(conn)) as ConnGuard)
        });
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE agent_sessions (session_id TEXT PRIMARY KEY);
             CREATE TABLE agent_messages (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, role TEXT NOT NULL, sequence INTEGER NOT NULL, content TEXT NOT NULL);",
        )
        .unwrap();
        SqliteJourneyRepository::ensure_schema(&conn).unwrap();
        ReviewJobRepository::ensure_schema(&conn).unwrap();
        drop(conn);
        let resolver = Arc::new(Resolver { shared: shared.clone(), pin: prov(), timeout: REVIEW_STAGE_TIMEOUT });
        Self { _dir: dir, path, shared, open, resolver }
    }

    fn conn(&self) -> Connection {
        let c = Connection::open(&self.path).unwrap();
        c.busy_timeout(Duration::from_secs(5)).unwrap();
        c
    }

    /// Session `sid` with a closing fork `f` (frozen 3..=5) and one queued review `r-{sid}` / job `job-{sid}`.
    fn seed(&self, sid: &str) -> String {
        let mut conn = self.conn();
        conn.execute("INSERT INTO agent_sessions VALUES (?1)", [sid]).unwrap();
        for (id, seq, role, text) in [
            ("anchor", 2, "user", "parent anchor message that is long enough".to_string()),
            ("f3", 3, "assistant", format!("{SECRET_LINE} 3")),
            ("f5", 5, "assistant", format!("{SECRET_LINE} 5")),
            ("future", 6, "assistant", "parent continuation after the anchor".to_string()),
        ] {
            conn.execute("INSERT INTO agent_messages VALUES (?1,?2,?3,?4,?5)", params![format!("{sid}-{id}"), sid, role, seq, text]).unwrap();
        }
        let review_id = format!("r-{sid}");
        let mut j = SessionJourney::new(sid, "main");
        j.branches.insert(
            "f".into(),
            JourneyFork {
                id: "f".into(),
                name: "fork f".into(),
                parent_branch_id: "main".into(),
                parent_anchor_message_id: Some(format!("{sid}-anchor")),
                anchor_sequence: 2,
                source_start_sequence: 3,
                state: ForkState::Closing,
                frozen_end_sequence: Some(5),
                close_work_id: None,
                handoff_capsule: None,
            },
        );
        j.reviews.insert(
            review_id.clone(),
            ReviewItem {
                id: review_id.clone(),
                fork_id: "f".into(),
                state: ReviewState::Queued,
                provenance: None,
                source_start_sequence: 3,
                source_end_sequence: 5,
                annotation: None,
                promoted_fact_ids: Vec::new(),
            },
        );
        j.revision = 1;
        SqliteJourneyRepository::compare_and_store(&mut conn, &j, 0).unwrap();
        for (id, seq, branch) in [("anchor", 2_i64, "main"), ("f3", 3, "f"), ("f5", 5, "f"), ("future", 6, "main")] {
            conn.execute(
                "INSERT INTO session_journey_memberships (session_id, message_id, sequence, branch_id, task_id) VALUES (?1, ?2, ?3, ?4, NULL)",
                params![sid, format!("{sid}-{id}"), seq, branch],
            )
            .unwrap();
        }
        ReviewJobRepository::enqueue(&conn, &j, format!("job-{sid}"), &review_id, &prov()).unwrap();
        review_id
    }

    async fn run(&self, gate: &Arc<tokio::sync::Mutex<()>>) -> bool {
        run_durable_job(&self.open, self.resolver.as_ref(), gate).await
    }

    fn job(&self, sid: &str) -> ReviewJob {
        ReviewJobRepository::get_by_job(&self.conn(), &format!("job-{sid}")).unwrap().unwrap()
    }

    fn review_state(&self, sid: &str) -> ReviewState {
        let snapshot = SessionJourneyApplicationService::snapshot(&self.conn(), sid).unwrap().snapshot;
        snapshot.reviews[&format!("r-{sid}")].state.clone()
    }

    fn capsule(&self, sid: &str) -> bool {
        let snapshot = SessionJourneyApplicationService::snapshot(&self.conn(), sid).unwrap().snapshot;
        snapshot.branches["f"].handoff_capsule.is_some()
    }

    fn outbox(&self, sid: &str) -> i64 {
        self.conn()
            .query_row("SELECT COUNT(*) FROM session_journey_embedding_outbox WHERE session_id=?1", [sid], |r| r.get(0))
            .unwrap_or(0)
    }

    fn retry(&self, sid: &str) {
        let mut conn = self.conn();
        let revision = SessionJourneyApplicationService::snapshot(&conn, sid).unwrap().revision;
        SessionJourneyApplicationService::retry_review(&mut conn, RetryReviewRequest { session_id: sid.into(), expected_revision: revision, review_id: format!("r-{sid}") }).unwrap();
    }
}

fn gate() -> Arc<tokio::sync::Mutex<()>> {
    Arc::new(tokio::sync::Mutex::new(()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_executors_sharing_the_gate_never_overlap_and_every_job_publishes_atomically() {
    let fx = Fx::new(Mode::Ok, 25);
    for sid in ["s1", "s2", "s3"] {
        fx.seed(sid);
    }
    let g = gate();
    let drain = |fx: &Fx, g: Arc<tokio::sync::Mutex<()>>| {
        let (open, resolver) = (fx.open.clone(), fx.resolver.clone());
        tokio::spawn(async move {
            while run_durable_job(&open, resolver.as_ref(), &g).await {}
        })
    };
    let (a, b) = (drain(&fx, g.clone()), drain(&fx, g.clone()));
    a.await.unwrap();
    b.await.unwrap();
    assert_eq!(fx.shared.max_in_flight.load(Ordering::SeqCst), 1, "provider calls must never overlap");
    assert_eq!(fx.shared.calls(), 9, "three stages per job, no duplicated work");
    for sid in ["s1", "s2", "s3"] {
        assert_eq!(fx.job(sid).state, ReviewJobState::Ready);
        assert_eq!(fx.review_state(sid), ReviewState::Ready);
        assert!(fx.capsule(sid));
        assert_eq!(fx.outbox(sid), 1);
        assert_eq!(fx.job(sid).attempt, 1);
    }
    assert!(!fx.run(&g).await, "a drained queue reports no work");
    assert_eq!(fx.outbox("s1"), 1, "no duplicate publication");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_row_blocks_a_second_claimer_even_with_a_different_gate() {
    let fx = Fx::new(Mode::Block, 0);
    fx.seed("s1");
    fx.seed("s2");
    let (open, resolver, g1) = (fx.open.clone(), fx.resolver.clone(), gate());
    let first = tokio::spawn(async move { run_durable_job(&open, resolver.as_ref(), &g1).await });
    fx.shared.entered.notified().await;
    assert_eq!(fx.job("s1").state, ReviewJobState::Running);
    assert!(!fx.run(&gate()).await, "second consumer must not claim while another job is running");
    assert_eq!(fx.job("s2").state, ReviewJobState::Queued);
    assert_eq!(fx.job("s2").attempt, 0);
    let mut conn = fx.conn();
    cancel_review_job(&mut conn, "s1", "job-s1").unwrap();
    assert!(first.await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_running_job_aborts_the_provider_blocks_late_publication_and_retry_runs_it_again() {
    let fx = Fx::new(Mode::Block, 0);
    fx.seed("s1");
    let (open, resolver, g) = (fx.open.clone(), fx.resolver.clone(), gate());
    let running = tokio::spawn(async move { run_durable_job(&open, resolver.as_ref(), &g).await });
    fx.shared.entered.notified().await;

    let mut conn = fx.conn();
    let cancelled = cancel_review_job(&mut conn, "s1", "job-s1").unwrap();
    assert_eq!(cancelled.state, ReviewJobState::Cancelled);
    assert!(running.await.unwrap());
    assert_eq!(fx.shared.in_flight.load(Ordering::SeqCst), 0, "the provider future was dropped");

    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Cancelled);
    assert!(job.error.as_deref().unwrap().contains("Cancelled by user"));
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);
    assert!(!fx.capsule("s1"));
    assert_eq!(fx.outbox("s1"), 0);
    assert!(!fx.run(&gate()).await, "a cancelled job is not claimable");

    fx.retry("s1");
    let retried = fx.job("s1");
    assert_eq!((retried.state, retried.attempt), (ReviewJobState::Queued, 0));
    fx.shared.set(Mode::Ok);
    assert!(fx.run(&gate()).await);
    assert_eq!(fx.job("s1").state, ReviewJobState::Ready);
    assert!(fx.capsule("s1"));
}

#[tokio::test]
async fn cancelling_a_queued_job_never_reaches_the_provider() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    let mut conn = fx.conn();
    assert_eq!(cancel_review_job(&mut conn, "s1", "job-s1").unwrap().state, ReviewJobState::Cancelled);
    assert!(!fx.run(&gate()).await);
    assert_eq!(fx.shared.calls(), 0);
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);
    assert!(cancel_review_job(&mut conn, "other-session", "job-s1").is_err(), "cancel is bound to the owner session");
}

#[tokio::test]
async fn provider_failure_is_visible_bounded_and_never_echoes_the_transcript() {
    let fx = Fx::new(Mode::Fail(format!("upstream 500 {SECRET_LINE} 3 {}", "x".repeat(2000))), 0);
    fx.seed("s1");
    assert!(fx.run(&gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    let error = job.error.unwrap();
    assert!(error.chars().count() <= ERROR_MAX_CHARS, "error is {} chars", error.chars().count());
    assert!(error.contains("Extract call failed"));
    assert!(!error.contains(SECRET_LINE), "transcript text must be redacted from the visible error");
    assert!(!error.contains('\n'));
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);
    assert!(!fx.capsule("s1"));

    fx.retry("s1");
    fx.shared.set(Mode::Ok);
    assert!(fx.run(&gate()).await);
    assert_eq!(fx.job("s1").state, ReviewJobState::Ready);
}

#[tokio::test]
async fn invalid_model_output_fails_closed_and_publishes_nothing() {
    let fx = Fx::new(Mode::Garbage, 0);
    fx.seed("s1");
    assert!(fx.run(&gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.unwrap().contains("invalid review structure"));
    assert!(!fx.capsule("s1"));
    assert_eq!(fx.outbox("s1"), 0);
}

#[tokio::test]
async fn evidence_outside_the_frozen_range_is_rejected_at_publication() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    fx.conn().execute("UPDATE session_journey_memberships SET branch_id='main' WHERE message_id='s1-f5'", []).unwrap();
    assert!(fx.run(&gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.unwrap().contains("Review result rejected"));
    assert!(!fx.capsule("s1"));
}

#[tokio::test]
async fn a_resolved_provider_with_other_provenance_is_never_substituted() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    let other = Arc::new(Resolver { shared: fx.shared.clone(), pin: RuntimeProvenance { model_id: "other-model".into(), ..prov() }, timeout: REVIEW_STAGE_TIMEOUT });
    assert!(run_durable_job(&fx.open, other.as_ref(), &gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.unwrap().contains("Provider resolution failed"));
    assert_eq!(fx.shared.calls(), 0);
    assert_eq!(job.model_id, "anthropic/claude-sonnet-5.5", "the pinned model is unchanged");
}

#[tokio::test]
async fn ownership_failures_fail_visibly_before_any_provider_call() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("gone");
    fx.conn().execute("DELETE FROM agent_sessions WHERE session_id='gone'", []).unwrap();
    assert!(fx.run(&gate()).await);
    let gone = fx.job("gone");
    assert_eq!(gone.state, ReviewJobState::Failed);
    assert!(gone.error.unwrap().contains("owner session no longer exists"));

    fx.seed("moved");
    {
        let mut conn = fx.conn();
        let snapshot = SessionJourneyApplicationService::snapshot(&conn, "moved").unwrap();
        let mut j = SqliteJourneyRepository::load(&conn, "moved").unwrap().unwrap();
        j.fail_review(snapshot.revision, "r-moved").unwrap();
        SqliteJourneyRepository::compare_and_store(&mut conn, &j, snapshot.revision).unwrap();
    }
    assert!(fx.run(&gate()).await);
    let moved = fx.job("moved");
    assert_eq!(moved.state, ReviewJobState::Failed);
    assert!(moved.error.unwrap().contains("no longer queued"));
    assert_eq!(fx.shared.calls(), 0);
}

#[tokio::test]
async fn orphaned_running_jobs_recover_after_restart_and_the_attempt_bound_fails_visibly() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    for expected_attempt in 1..=MAX_CLAIM_ATTEMPTS {
        let mut conn = fx.conn();
        let claimed = ReviewJobRepository::claim(&mut conn, "crashing", STALE_LEASE_SECS).unwrap().unwrap();
        assert_eq!(claimed.attempt, expected_attempt);
        assert_eq!(ReviewJobRepository::requeue_orphaned_running_jobs(&conn).unwrap(), 1, "startup recovery requeues the orphan");
    }
    // Exhausted jobs are failed before claiming, so nothing is claimed and the call reports no work.
    assert!(!fx.run(&gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.as_deref().unwrap().contains("Attempt limit reached"));
    assert_eq!(fx.shared.calls(), 0, "an exhausted job never reaches the provider");
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);

    fx.retry("s1");
    assert_eq!(fx.job("s1").attempt, 0, "an explicit retry resets the attempt counter");
    assert!(fx.run(&gate()).await);
    assert_eq!(fx.job("s1").state, ReviewJobState::Ready);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn executor_shutdown_mid_job_is_recovered_by_the_next_startup() {
    let fx = Fx::new(Mode::Block, 0);
    fx.seed("s1");
    let runner = Arc::new(DurableReviewJobRunner { open: fx.open.clone(), resolver: fx.resolver.clone(), gate: gate() });
    let first = JourneyReviewExecutorHandle::spawn_with_runner(runner);
    first.wake();
    fx.shared.entered.notified().await;
    first.shutdown().await;
    let stuck = fx.job("s1");
    assert_eq!(stuck.state, ReviewJobState::Running, "a dropped in-flight job stays running until recovery");

    assert_eq!(ReviewJobRepository::requeue_orphaned_running_jobs(&fx.conn()).unwrap(), 1);
    fx.shared.set(Mode::Ok);
    let runner = Arc::new(DurableReviewJobRunner { open: fx.open.clone(), resolver: fx.resolver.clone(), gate: gate() });
    let second = JourneyReviewExecutorHandle::spawn_with_runner(runner);
    second.wake();
    wait_for(|| fx.job("s1").state == ReviewJobState::Ready).await;
    assert_eq!(fx.job("s1").attempt, 2);
    assert!(fx.capsule("s1"));
    second.shutdown().await;
}

async fn wait_for(mut done: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition not reached");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_executor_drains_a_backlog_beyond_one_turn_and_then_goes_quiet() {
    let fx = Fx::new(Mode::Ok, 0);
    let sids: Vec<String> = (1..=MAX_JOBS_PER_WAKE + 2).map(|n| format!("s{n}")).collect();
    for sid in &sids {
        fx.seed(sid);
    }
    let runner = Arc::new(DurableReviewJobRunner { open: fx.open.clone(), resolver: fx.resolver.clone(), gate: gate() });
    let handle = JourneyReviewExecutorHandle::spawn_with_runner(runner);
    handle.wake();
    wait_for(|| sids.iter().all(|sid| fx.job(sid).state == ReviewJobState::Ready)).await;
    let opens_after_drain = fx.shared.opens.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let opens_idle = fx.shared.opens.load(Ordering::SeqCst);
    assert!(opens_idle <= opens_after_drain + 4, "idle executor opened the database {} extra times", opens_idle - opens_after_drain);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_queue_is_not_polled_in_a_loop() {
    let fx = Fx::new(Mode::Ok, 0);
    let runner = Arc::new(DurableReviewJobRunner { open: fx.open.clone(), resolver: fx.resolver.clone(), gate: gate() });
    let handle = JourneyReviewExecutorHandle::spawn_with_runner(runner);
    handle.wake();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let opens = fx.shared.opens.load(Ordering::SeqCst);
    assert!(opens <= 6, "empty queue opened the database {opens} times in 500 ms");
    for _ in 0..50 {
        handle.wake();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(fx.shared.opens.load(Ordering::SeqCst) <= opens + 6, "coalesced wakes must not multiply work");
    handle.shutdown().await;
}

#[tokio::test]
async fn a_scripted_failure_then_success_sequence_ends_ready_after_one_retry() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    fx.shared.script.lock().unwrap().push_back(Mode::Fail("temporary outage".into()));
    fx.shared.script.lock().unwrap().push_back(Mode::Ok);
    assert!(fx.run(&gate()).await);
    assert_eq!(fx.job("s1").state, ReviewJobState::Failed);
    assert!(fx.job("s1").error.unwrap().contains("temporary outage"));
    fx.retry("s1");
    assert!(fx.run(&gate()).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Ready, "error after retry: {:?}", job.error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hung_provider_times_out_visibly_and_releases_the_single_executor() {
    let fx = Fx::new(Mode::Block, 0);
    fx.seed("s1");
    fx.seed("s2");
    let slow = Arc::new(Resolver { shared: fx.shared.clone(), pin: prov(), timeout: Duration::from_millis(300) });
    let g = gate();
    assert!(run_durable_job(&fx.open, slow.as_ref(), &g).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.as_deref().unwrap().contains("timed out"), "visible timeout: {:?}", job.error);
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);
    assert_eq!(fx.shared.in_flight.load(Ordering::SeqCst), 0, "the stalled provider future was dropped");
    fx.shared.set(Mode::Ok);
    assert!(run_durable_job(&fx.open, slow.as_ref(), &g).await, "the next job is claimable after the timeout");
    assert_eq!(fx.job("s2").state, ReviewJobState::Ready);
}

#[test]
fn a_review_id_owned_by_another_session_is_a_conflict_not_a_foreign_job() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    fx.seed("s2");
    let conn = fx.conn();
    let mut journey = SqliteJourneyRepository::load(&conn, "s2").unwrap().unwrap();
    let item = journey.reviews["r-s2"].clone();
    journey.reviews.insert("r-s1".into(), ReviewItem { id: "r-s1".into(), ..item });
    let error = ReviewJobRepository::enqueue(&conn, &journey, "job-x", "r-s1", &prov()).unwrap_err();
    assert!(matches!(error, QueueError::Conflict(_)), "got {error:?}");
    let original = fx.job("s1");
    assert_eq!(original.session_id, "s1", "the first session's job is untouched");
    let same = SqliteJourneyRepository::load(&conn, "s1").unwrap().unwrap();
    let again = ReviewJobRepository::enqueue(&conn, &same, "job-s1", "r-s1", &prov()).unwrap();
    assert_eq!(again.job_id, "job-s1", "the owner's own replay stays idempotent");
}

#[test]
fn a_finished_attempt_cannot_fail_a_job_that_was_cancelled_or_retried_meanwhile() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    let mut conn = fx.conn();
    let error = commit_failed_review(&mut conn, "job-s1", "s1", "r-s1", "late failure", true).unwrap_err();
    assert!(matches!(error, QueueError::Conflict(_)), "a queued job is not the attempt that ran: {error:?}");
    assert_eq!(fx.job("s1").state, ReviewJobState::Queued);
    assert_eq!(fx.review_state("s1"), ReviewState::Queued);
}

struct HungResolver(Duration);

#[async_trait]
impl JourneyReviewProviderResolver for HungResolver {
    async fn resolve(&self, _session_id: &str, _expected: &RuntimeProvenance) -> Result<Arc<dyn JourneyUnboundedReviewProvider>, String> {
        std::future::pending::<()>().await;
        unreachable!()
    }

    fn stage_timeout(&self) -> Duration {
        self.0
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hung_provider_resolution_times_out_visibly_and_releases_the_single_executor() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    fx.seed("s2");
    let g = gate();
    let hung = HungResolver(Duration::from_millis(300));
    assert!(run_durable_job(&fx.open, &hung, &g).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed);
    assert!(job.error.as_deref().unwrap().contains("resolution timed out"), "visible timeout: {:?}", job.error);
    assert_eq!(fx.review_state("s1"), ReviewState::Failed);
    assert_eq!(fx.shared.calls(), 0, "no provider call was ever made");
    assert!(run_durable_job(&fx.open, fx.resolver.as_ref(), &g).await, "the gate was released for the next job");
    assert_eq!(fx.job("s2").state, ReviewJobState::Ready);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hung_provider_resolution_honours_a_user_cancel_immediately() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    let g = gate();
    let hung = Arc::new(HungResolver(Duration::from_secs(600)));
    let (open, h2, g2) = (fx.open.clone(), hung.clone(), g.clone());
    let run = tokio::spawn(async move { run_durable_job(&open, h2.as_ref(), &g2).await });
    let started = std::time::Instant::now();
    loop {
        if fx.job("s1").state == ReviewJobState::Running {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "job never started");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut conn = fx.conn();
    cancel_review_job(&mut conn, "s1", "job-s1").unwrap();
    drop(conn);
    tokio::time::timeout(Duration::from_secs(10), run).await.expect("cancel must abort the hung resolver well before its timeout").unwrap();
    assert_eq!(fx.job("s1").state, ReviewJobState::Cancelled);
    assert_eq!(fx.shared.calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreadable_journey_still_fails_the_job_visibly_instead_of_leaving_it_running() {
    let fx = Fx::new(Mode::Ok, 0);
    fx.seed("s1");
    fx.conn().execute("UPDATE session_journeys SET state_json='not json' WHERE session_id='s1'", []).unwrap();
    let g = gate();
    assert!(fx.run(&g).await);
    let job = fx.job("s1");
    assert_eq!(job.state, ReviewJobState::Failed, "got {:?} / {:?}", job.state, job.error);
    assert!(job.error.as_deref().is_some_and(|e| !e.is_empty()), "the failure reason is visible");
    assert_eq!(fx.shared.calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unrelated_journey_revision_bump_during_the_review_does_not_discard_the_result() {
    let fx = Fx::new(Mode::Ok, 150);
    fx.seed("s1");
    let g = gate();
    let (open, resolver, g2) = (fx.open.clone(), fx.resolver.clone(), g.clone());
    let run = tokio::spawn(async move { run_durable_job(&open, resolver.as_ref(), &g2).await });
    let started = std::time::Instant::now();
    while fx.shared.calls() == 0 {
        assert!(started.elapsed() < Duration::from_secs(10), "provider never called");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let mut conn = fx.conn();
        let mut journey = SqliteJourneyRepository::load(&conn, "s1").unwrap().unwrap();
        let old = journey.revision;
        journey.revision = old + 1;
        SqliteJourneyRepository::compare_and_store(&mut conn, &journey, old).unwrap();
    }
    assert!(run.await.unwrap());
    assert_eq!(fx.job("s1").state, ReviewJobState::Ready, "got {:?}", fx.job("s1").error);
    assert_eq!(fx.review_state("s1"), ReviewState::Ready);
    assert!(fx.capsule("s1"));
}
