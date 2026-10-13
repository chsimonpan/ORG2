//! Pins the REAL current behaviour of Journey x Task-fork / app-service fork ownership.
//!
//! Child module of `task_fork_tests` (registered there with a `#[path]` line) so it can reuse the
//! private `setup`/`put`/`anchor_for`/`Fake` helpers. Nothing here changes production behaviour;
//! where reading the code disagreed with the original hypothesis the ACTUAL behaviour is asserted
//! and marked with an `AUDIT:` comment.
use super::*;
use crate::core::session::journey_application_service::{
    CreateCheckpointRequest, CreateForkRequest, JourneyApplicationError,
    SessionJourneyApplicationService,
};
use crate::core::session::journey_lifecycle::{SqliteJourneyRepository, TaskState};
use crate::core::session::journey_runtime_persistence::assign_inserted_message;

const M_U1: &str = "m-u1";
const M_A1: &str = "m-a1";
const F_U1: &str = "f-u1";
const F_A1: &str = "f-a1";

/// Insert a message and register its Journey membership the way the real insert path does
/// (`insert_message` -> `assign_inserted_message`). The shared `put` helper does NOT do this.
fn put_j(session: &str, id: &str, role: &str, content: &str) -> i64 {
    let seq = put(session, id, role, content);
    let conn = get_connection().unwrap();
    assign_inserted_message(&conn, session, id, seq, role).unwrap();
    seq
}

/// Parent with a real Journey: main branch holds m-u1 (seq 0) and m-a1 (seq 1); a fork `f1`
/// anchored at m-u1 holds f-u1 (seq 2) and f-a1 (seq 3). m-a1 is therefore inactive-branch
/// (hidden from the provider) while m-u1/f-u1/f-a1 are visible.
fn journey_parent() {
    put(PARENT, M_U1, "user", "MARK-main-user");
    put(PARENT, M_A1, "assistant", "MARK-inactive-assistant");
    {
        let conn = get_connection().unwrap();
        SqliteJourneyRepository::ensure_schema(&conn).unwrap();
        for (id, seq) in [(M_U1, 0i64), (M_A1, 1i64)] {
            conn.execute(
                "INSERT INTO session_journey_memberships(session_id,message_id,sequence,branch_id,task_id) VALUES(?1,?2,?3,'main',NULL)",
                params![PARENT, id, seq],
            )
            .unwrap();
        }
    }
    {
        let mut conn = get_connection().unwrap();
        let r = SessionJourneyApplicationService::create_fork(
            &mut conn,
            CreateForkRequest {
                session_id: PARENT.into(),
                expected_revision: 0,
                fork_id: "f1".into(),
                task_id: "ft1".into(),
                task_name: "分叉任务".into(),
                anchor_message_id: Some(M_U1.into()),
            },
        )
        .unwrap();
        assert_eq!(r.revision, 1);
    }
    put_j(PARENT, F_U1, "user", "MARK-fork-user");
    put_j(PARENT, F_A1, "assistant", "MARK-fork-assistant");
}

fn journey_state_json(session: &str) -> Option<String> {
    get_connection()
        .unwrap()
        .query_row(
            "SELECT state_json FROM session_journeys WHERE session_id=?1",
            [session],
            |r| r.get(0),
        )
        .ok()
}

fn memberships(session: &str) -> Vec<(String, i64, String, Option<String>)> {
    let conn = get_connection().unwrap();
    let mut stmt = conn
        .prepare("SELECT message_id,sequence,branch_id,task_id FROM session_journey_memberships WHERE session_id=?1 ORDER BY sequence,message_id")
        .unwrap();
    let rows = stmt
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows
}

fn provider_history_text(session: &str) -> String {
    let history = crate::core::session::persistence::load_llm_history(session).unwrap();
    serde_json::to_string(&history).unwrap()
}

// 3. Task fork seed + summarizer transcript vs inactive Journey branches.
#[tokio::test]
async fn task_fork_seed_and_transcript_vs_inactive_branches() {
    let _s = setup();
    journey_parent();

    // Precondition: the parent's own provider view hides the inactive-branch row.
    let visible = provider_history_text(PARENT);
    assert!(visible.contains("MARK-main-user"), "{visible}");
    assert!(visible.contains("MARK-fork-user"), "{visible}");
    assert!(visible.contains("MARK-fork-assistant"), "{visible}");
    assert!(!visible.contains("MARK-inactive-assistant"), "{visible}");

    let state_before = journey_state_json(PARENT).unwrap();
    let members_before = memberships(PARENT);
    assert_eq!(members_before.len(), 4);

    let fake = Fake::new(Mode::Ok("压缩摘要：Journey 夹具".into()));
    let anchor = anchor_for(PARENT, F_A1);
    let view = create_task_fork(PARENT, req("jf-intent-1", anchor), fake.as_ref())
        .await
        .unwrap();
    assert!(view.newly_published);
    assert_eq!(fake.calls(), 1);
    assert_eq!(view.link.anchor_message_id, F_A1);
    assert_eq!(view.link.anchor_sequence, 3);

    // AUDIT: the summarizer transcript is built from ALL parent rows up to the anchor (compaction
    // filter only) - it includes the inactive-branch row the parent's provider view hides.
    let transcript = fake.last_transcript.lock().unwrap().clone();
    for marker in [
        "MARK-main-user",
        "MARK-inactive-assistant",
        "MARK-fork-user",
        "MARK-fork-assistant",
    ] {
        assert!(transcript.contains(marker), "missing {marker}: {transcript}");
    }

    // The child holds exactly the seed row and carries no Journey of its own.
    let child = view.link.child_session_id.clone().unwrap();
    assert_ne!(child, PARENT);
    let rows = load_rows(&get_connection().unwrap(), &child, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, format!("task-seed:{}", view.link.id));
    assert_eq!(rows[0].sequence, 0);
    assert!(rows[0].content.starts_with(SEED_MESSAGE_PREFIX));
    assert!(!rows[0].content.contains("MARK-inactive-assistant"));
    assert!(journey_state_json(&child).is_none());
    assert!(memberships(&child).is_empty());

    // AUDIT: the stored anchor is a session-namespace anchor (`session:{owner}`), never a Journey
    // branch id; no Journey/membership check happens at Task-fork creation.
    let anchor_json: String = get_connection()
        .unwrap()
        .query_row(
            "SELECT anchor_json FROM task_forks WHERE id=?1",
            [&view.link.id],
            |r| r.get(0),
        )
        .unwrap();
    let a: serde_json::Value = serde_json::from_str(&anchor_json).unwrap();
    assert_eq!(a["branchId"], format!("session:{PARENT}"));
    assert_eq!(a["ownerSessionId"], PARENT);
    assert_eq!(a["sourceSessionId"], PARENT);
    assert_eq!(a["durableMessageId"], F_A1);

    // Creating a Task fork never touches the parent's Journey rows.
    assert_eq!(journey_state_json(PARENT).unwrap(), state_before);
    assert_eq!(memberships(PARENT), members_before);

    // AUDIT: a Task fork may be anchored at a row the parent's Journey hides (m-a1, inactive
    // branch). Creation succeeds and its transcript stops at that row.
    let fake2 = Fake::new(Mode::Ok("压缩摘要：隐藏锚点".into()));
    let hidden = anchor_for(PARENT, M_A1);
    let view2 = create_task_fork(PARENT, req("jf-intent-hidden", hidden), fake2.as_ref())
        .await
        .unwrap();
    assert!(view2.newly_published);
    assert_eq!(view2.link.anchor_message_id, M_A1);
    assert_eq!(view2.link.anchor_sequence, 1);
    let transcript2 = fake2.last_transcript.lock().unwrap().clone();
    assert!(transcript2.contains("MARK-inactive-assistant"), "{transcript2}");
    assert!(!transcript2.contains("MARK-fork-user"), "{transcript2}");
    assert_ne!(view2.link.child_session_id, view.link.child_session_id);
    assert_eq!(journey_state_json(PARENT).unwrap(), state_before);
}

// 4. Same intent / different anchor: rejected or replayed, across Task fork + app service.
#[tokio::test]
async fn same_intent_different_anchor_is_rejected_or_replayed() {
    let _s = setup();
    journey_parent();
    let state_before = journey_state_json(PARENT).unwrap();

    let fake = Fake::new(Mode::Ok("压缩摘要：意图重放".into()));
    let a_fork = anchor_for(PARENT, F_A1);
    let first = create_task_fork(PARENT, req("jf-same", a_fork.clone()), fake.as_ref())
        .await
        .unwrap();
    assert!(first.newly_published);
    assert_eq!(fake.calls(), 1);
    let child = first.link.child_session_id.clone().unwrap();

    // Same intent + same anchor: idempotent replay, no new model call, same child.
    let again = create_task_fork(PARENT, req("jf-same", a_fork.clone()), fake.as_ref())
        .await
        .unwrap();
    assert!(!again.newly_published);
    assert_eq!(again.link.id, first.link.id);
    assert_eq!(again.link.child_session_id.as_deref(), Some(child.as_str()));
    assert_eq!(fake.calls(), 1);

    // Same intent + different anchor: rejected.
    let a_other = anchor_for(PARENT, M_U1);
    let err = create_task_fork(PARENT, req("jf-same", a_other), fake.as_ref())
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(err.contains("task_fork_intent_conflict"), "{err}");
    assert_eq!(fake.calls(), 1);

    // Parent changes (new durable row on the active fork branch).
    put_j(PARENT, "f-u2", "user", "MARK-fork-user-2");
    // AUDIT: replay of the SAME intent + SAME (now stale) anchor still succeeds as an idempotent
    // replay - the existing-link check precedes anchor revalidation.
    let replay_stale = create_task_fork(PARENT, req("jf-same", a_fork.clone()), fake.as_ref())
        .await
        .unwrap();
    assert!(!replay_stale.newly_published);
    assert_eq!(replay_stale.link.child_session_id.as_deref(), Some(child.as_str()));
    assert_eq!(fake.calls(), 1);
    // A NEW intent with the stale anchor is rejected by revalidation.
    let err = create_task_fork(PARENT, req("jf-new", a_fork), fake.as_ref())
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(err.contains("journey_anchor_changed"), "{err}");
    assert_eq!(fake.calls(), 1);

    // App service: no intent concept. A duplicate fork id is REJECTED (not replayed) and a stale
    // expected_revision is a CAS conflict. The only state change so far is f-u2's membership
    // (the active task is still in Active state, so no revision bump).
    let state_mid = journey_state_json(PARENT).unwrap();
    assert_eq!(state_mid, state_before, "user row on an Active task must not bump the revision");
    {
        let mut conn = get_connection().unwrap();
        let dup = SessionJourneyApplicationService::create_fork(
            &mut conn,
            CreateForkRequest {
                session_id: PARENT.into(),
                expected_revision: 1,
                fork_id: "f1".into(),
                task_id: "ft-other".into(),
                task_name: "重复".into(),
                anchor_message_id: Some(F_U1.into()),
            },
        )
        .unwrap_err();
        match dup {
            JourneyApplicationError::校验失败(m) => assert!(m.contains("已存在"), "{m}"),
            other => panic!("expected 校验失败 duplicate id, got {other:?}"),
        }
        // A fresh id while a task is still active is also rejected (ActiveTaskExists).
        let busy = SessionJourneyApplicationService::create_fork(
            &mut conn,
            CreateForkRequest {
                session_id: PARENT.into(),
                expected_revision: 1,
                fork_id: "f2".into(),
                task_id: "ft2".into(),
                task_name: "再叉".into(),
                anchor_message_id: Some(F_U1.into()),
            },
        );
        assert!(busy.is_err());
        // Stale expected revision.
        let stale = SessionJourneyApplicationService::create_fork(
            &mut conn,
            CreateForkRequest {
                session_id: PARENT.into(),
                expected_revision: 0,
                fork_id: "f3".into(),
                task_id: "ft3".into(),
                task_name: "旧修订".into(),
                anchor_message_id: Some(F_U1.into()),
            },
        )
        .unwrap_err();
        match stale {
            JourneyApplicationError::修订冲突 { expected, actual } => {
                assert_eq!((expected, actual), (0, 1));
            }
            other => panic!("expected 修订冲突, got {other:?}"),
        }
        // Checkpoint anchored on a row of the OTHER (inactive main) branch is rejected.
        let cp = SessionJourneyApplicationService::create_checkpoint(
            &mut conn,
            CreateCheckpointRequest {
                session_id: PARENT.into(),
                expected_revision: 1,
                checkpoint_id: "cp-hidden".into(),
                name: "隐藏行".into(),
                message_id: M_A1.into(),
            },
        );
        assert!(cp.is_err(), "checkpoint on a row outside the active task must be rejected");
    }
    // Every rejected call left the Journey untouched.
    let journey = SqliteJourneyRepository::load(&get_connection().unwrap(), PARENT)
        .unwrap()
        .unwrap();
    assert_eq!(journey.revision, 1);
    assert_eq!(journey.active_branch_id, "f1");
    assert_eq!(journey.tasks["ft1"].state, TaskState::Active);
    assert_eq!(journey_state_json(PARENT).unwrap(), state_before);
}
