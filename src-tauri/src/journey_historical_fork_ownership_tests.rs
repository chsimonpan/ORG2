//! Native-crate pins for Journey x historical-fork ownership (main session vs fork child).
//!
//! Child module of `registered_owner_ipc_tests` (registered there via `#[path]`). Real
//! `historical_runtime_fork` IPC + the real Journey application service / membership hook; the
//! provider-visible view is read with `provider_visible_rows` (the same projection `load_llm_history`
//! applies) because the agent-core db bridge is not registered in native tests.
//!
//! Where reading the code disagreed with the original hypothesis, the ACTUAL behaviour is asserted
//! and tagged `AUDIT:`. Nothing here changes production behaviour.
use super::super::*;
use super::invoke;
use agent_core::core::session::journey_application_service as svc;
use agent_core::session::journey_lifecycle::{SqliteJourneyRepository, TaskState};
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

type Rows = Vec<(String, i64, String)>;
type Members = Vec<(String, i64, String, Option<String>)>;

fn ts(seq: i64) -> String {
    format!("2026-10-13T00:00:{:02}Z", seq % 60)
}

fn put(session: &str, id: &str, role: &str, content: &str, seq: i64) {
    let c = database::db::get_connection().unwrap();
    c.execute(
        "INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![id, session, role, content, seq, ts(seq)],
    )
    .unwrap();
}

/// Insert + register the Journey membership exactly like the real insert path does.
fn put_j(session: &str, id: &str, role: &str, content: &str, seq: i64) {
    put(session, id, role, content, seq);
    let c = database::db::get_connection().unwrap();
    agent_core::session::journey_runtime_persistence::assign_inserted_message(&c, session, id, seq, role).unwrap();
}

fn max_seq(session: &str) -> i64 {
    let c = database::db::get_connection().unwrap();
    c.query_row("SELECT COALESCE(MAX(sequence),0) FROM agent_messages WHERE session_id=?1", [session], |r| r.get(0)).unwrap()
}

/// Registered main session with a REAL Journey: main = {m-u1(1), m-a1(2)}, fork f1 anchored at
/// m-u1 = {f-u1(3), f-a1(4)}. m-a1 is therefore hidden from the provider; m-u1/f-u1/f-a1 visible.
fn seed_journey_parent(state: &agent_core::state::AgentAppState, owner: &str) {
    tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(
        owner.into(),
        agent_core::definitions::sde_agent(),
    )));
    {
        let c = database::db::get_connection().unwrap();
        agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
        c.execute(
            "INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-13T00:00:00Z','2026-10-13T00:00:00Z')",
            [owner],
        )
        .unwrap();
    }
    put(owner, "m-u1", "user", "MARK-main-user", 1);
    put(owner, "m-a1", "assistant", "MARK-inactive-assistant", 2);
    {
        let c = database::db::get_connection().unwrap();
        SqliteJourneyRepository::ensure_schema(&c).unwrap();
        for (id, seq) in [("m-u1", 1i64), ("m-a1", 2i64)] {
            c.execute(
                "INSERT INTO session_journey_memberships(session_id,message_id,sequence,branch_id,task_id) VALUES(?1,?2,?3,'main',NULL)",
                rusqlite::params![owner, id, seq],
            )
            .unwrap();
        }
    }
    {
        let mut c = database::db::get_connection().unwrap();
        let r = svc::SessionJourneyApplicationService::create_fork(
            &mut c,
            svc::CreateForkRequest {
                session_id: owner.into(),
                expected_revision: 0,
                fork_id: "f1".into(),
                task_id: "ft1".into(),
                task_name: "fork task".into(),
                anchor_message_id: Some("m-u1".into()),
            },
        )
        .unwrap();
        assert_eq!(r.revision, 1);
    }
    put_j(owner, "f-u1", "user", "MARK-fork-user", 3);
    put_j(owner, "f-a1", "assistant", "MARK-fork-assistant", 4);
}

/// Give a (journey-less) historical child its own Journey through the real service + hook:
/// pending task, then two new durable rows that the hook assigns to branch `main` / task `ct1`.
fn give_child_journey(child: &str) {
    {
        let mut c = database::db::get_connection().unwrap();
        svc::SessionJourneyApplicationService::create_task(
            &mut c,
            svc::CreateTaskRequest {
                session_id: child.into(),
                expected_revision: 0,
                task_id: "ct1".into(),
                name: "child task".into(),
                position: svc::TaskStartPosition::下一条用户消息,
            },
        )
        .unwrap();
    }
    let next = max_seq(child) + 1;
    put_j(child, "child-new-u1", "user", "MARK-child-user", next);
    put_j(child, "child-new-a1", "assistant", "MARK-child-assistant", next + 1);
}

fn window(state: &agent_core::state::AgentAppState) -> (tauri::App<MockRuntime>, tauri::WebviewWindow<MockRuntime>) {
    let app = mock_builder()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            historical_runtime_fork,
            journey_owner_open,
            journey_domain_snapshot,
            journey_boundary_preview,
            journey_task_start
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    (app, w)
}

fn anchor_json(owner: &str, message: &str) -> Value {
    let c = database::db::get_connection().unwrap();
    serde_json::to_value(
        agent_core::core::session::journey_canonical::resolve_with_connection(
            &c,
            owner,
            owner,
            &format!("session:{owner}"),
            message,
            max_seq(owner) as u64,
        )
        .unwrap(),
    )
    .unwrap()
}

fn hfork_anchor(w: &tauri::WebviewWindow<MockRuntime>, owner: &str, anchor: Value, intent: &str) -> Result<Value, Value> {
    invoke(w, "historical_runtime_fork", json!({"ownerSessionId":owner,"request":{"anchor":anchor,"intentId":intent}}))
}
fn hfork(w: &tauri::WebviewWindow<MockRuntime>, owner: &str, message: &str, intent: &str) -> Result<Value, Value> {
    hfork_anchor(w, owner, anchor_json(owner, message), intent)
}
fn child_of(v: &Value) -> String {
    v["childSessionId"].as_str().unwrap().to_string()
}

fn rows(session: &str) -> Rows {
    let c = database::db::get_connection().unwrap();
    let mut s = c
        .prepare("SELECT id,sequence,content FROM agent_messages WHERE session_id=?1 ORDER BY sequence,id")
        .unwrap();
    let v = s
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    v
}
fn members(session: &str) -> Members {
    let c = database::db::get_connection().unwrap();
    let mut s = c
        .prepare("SELECT message_id,sequence,branch_id,task_id FROM session_journey_memberships WHERE session_id=?1 ORDER BY sequence,message_id")
        .unwrap();
    let v = s
        .query_map([session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    v
}
fn state_json(session: &str) -> Option<String> {
    let c = database::db::get_connection().unwrap();
    c.query_row("SELECT state_json FROM session_journeys WHERE session_id=?1", [session], |r| r.get(0)).ok()
}
fn snap(session: &str) -> (Option<String>, Members, Rows) {
    (state_json(session), members(session), rows(session))
}
fn visible_ids(session: &str) -> Vec<String> {
    let c = rusqlite::Connection::open(database::db::get_db_path()).unwrap();
    let all = agent_core::foundation::persistence::db_helpers::load_messages("agent", session).unwrap();
    let (visible, capsules) =
        agent_core::session::journey_runtime_persistence::provider_visible_rows(&c, session, all).unwrap();
    assert!(capsules.is_empty());
    visible.into_iter().map(|r| r.id).collect()
}
fn parent_of(session: &str) -> Option<String> {
    let c = database::db::get_connection().unwrap();
    c.query_row("SELECT parent_session_id FROM agent_sessions WHERE session_id=?1", [session], |r| r.get(0)).unwrap()
}
fn journey_of(session: &str) -> Option<agent_core::session::journey_lifecycle::SessionJourney> {
    let c = rusqlite::Connection::open(database::db::get_db_path()).unwrap();
    SqliteJourneyRepository::load_existing(&c, session).unwrap()
}
fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// 1. Row visibility of a historical child vs its Journey parent.
#[test]
fn parent_with_journey_historical_fork_row_visibility() {
    let _sandbox = test_helpers::test_env::sandbox();
    let owner = "osagent-jfo-visibility";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, owner);
    let (_app, w) = window(&state);

    // Parent's own provider view hides the inactive-branch row m-a1.
    assert_eq!(visible_ids(owner), strs(&["m-u1", "f-u1", "f-a1"]));

    let result = hfork(&w, owner, "f-a1", "vis-1").unwrap();
    let child = child_of(&result);
    assert!(child.starts_with(&format!("{owner}-history-")));
    assert_eq!(parent_of(&child).as_deref(), Some(owner));

    // Child holds a physical copy of every prefix row (incl. the one the parent hides) under child-local ids.
    let child_ids: Vec<String> = rows(&child).into_iter().map(|r| r.0).collect();
    assert_eq!(
        child_ids,
        ["m-u1", "m-a1", "f-u1", "f-a1"].iter().map(|m| format!("{child}:{m}")).collect::<Vec<_>>()
    );
    // AUDIT: no Journey row / membership is copied, so the child's provider view is UNFILTERED:
    // the row the parent's Journey hides (m-a1) IS visible to the child's model.
    assert!(journey_of(&child).is_none());
    assert!(members(&child).is_empty());
    assert!(state_json(&child).is_none());
    assert_eq!(visible_ids(&child), child_ids);
    assert!(visible_ids(&child).contains(&format!("{child}:m-a1")));
    // The parent is unaffected.
    assert_eq!(visible_ids(owner), strs(&["m-u1", "f-u1", "f-a1"]));

    // AUDIT: a historical fork may be anchored at a row hidden by the parent's Journey (m-a1);
    // the endpoint is a physical sequence, not a Journey-visible one.
    let hidden = hfork(&w, owner, "m-a1", "vis-hidden").unwrap();
    let hidden_child = child_of(&hidden);
    assert_ne!(hidden_child, child);
    assert_eq!(
        rows(&hidden_child).into_iter().map(|r| r.0).collect::<Vec<_>>(),
        ["m-u1", "m-a1"].iter().map(|m| format!("{hidden_child}:{m}")).collect::<Vec<_>>()
    );
}

// 2. The parent's Journey (state, memberships, rows) is untouched by a historical fork, and the
//    child's and parent's later Journey writes do not leak into each other.
#[test]
fn parent_journey_unchanged_by_historical_fork() {
    let _sandbox = test_helpers::test_env::sandbox();
    let owner = "osagent-jfo-unchanged";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, owner);
    let (_app, w) = window(&state);

    let before = snap(owner);
    assert!(before.0.is_some());
    assert_eq!(before.1.len(), 4);
    let child = child_of(&hfork(&w, owner, "f-a1", "unch-1").unwrap());
    assert_eq!(snap(owner), before, "historical fork must not mutate the parent's Journey/memberships/rows");

    // Child starts its own Journey and writes: only newly assigned rows are provider-visible.
    give_child_journey(&child);
    let cj = journey_of(&child).unwrap();
    assert_eq!(cj.active_branch_id, "main");
    assert_eq!(cj.active_task_id.as_deref(), Some("ct1"));
    assert_eq!(cj.tasks["ct1"].state, TaskState::Active);
    assert_eq!(members(&child).len(), 2, "only the two newly written child rows carry memberships");
    // AUDIT: once the child has any membership, the copied prefix rows (no membership) are DENIED.
    assert_eq!(visible_ids(&child), strs(&["child-new-u1", "child-new-a1"]));
    assert_eq!(snap(owner), before, "child Journey writes must not touch the parent");

    // Parent keeps writing + checkpointing; the child's Journey is untouched.
    let child_after_child_writes = snap(&child);
    let next = max_seq(owner) + 1;
    put_j(owner, "p-late-a", "assistant", "MARK-parent-late", next);
    {
        let mut c = database::db::get_connection().unwrap();
        let r = svc::SessionJourneyApplicationService::create_checkpoint(
            &mut c,
            svc::CreateCheckpointRequest {
                session_id: owner.into(),
                expected_revision: 1,
                checkpoint_id: "cp-parent".into(),
                name: "parent checkpoint".into(),
                message_id: "f-a1".into(),
            },
        )
        .unwrap();
        assert_eq!(r.revision, 2);
    }
    assert_ne!(state_json(owner), before.0, "the parent Journey did advance");
    assert_eq!(snap(&child), child_after_child_writes);
    assert_eq!(visible_ids(owner), strs(&["m-u1", "f-u1", "f-a1", "p-late-a"]));
    assert_eq!(visible_ids(&child), strs(&["child-new-u1", "child-new-a1"]));
}

// 4 (historical flavour). Intent semantics: same intent+anchor replays; same intent+different anchor
//    is rejected; a replay after the parent changed still returns the original child.
#[test]
fn historical_same_intent_different_anchor_is_rejected_or_replayed() {
    let _sandbox = test_helpers::test_env::sandbox();
    let owner = "osagent-jfo-intent";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, owner);
    let (_app, w) = window(&state);

    let anchor = anchor_json(owner, "f-a1");
    let first = hfork_anchor(&w, owner, anchor.clone(), "same").unwrap();
    assert_eq!(hfork_anchor(&w, owner, anchor.clone(), "same").unwrap(), first, "same intent+anchor is an idempotent replay");

    let other = anchor_json(owner, "f-u1");
    let err = hfork_anchor(&w, owner, other, "same").unwrap_err().to_string();
    assert!(err.contains("historical_fork_intent_conflict"), "{err}");

    // Parent changes (new durable row): the old anchor is now stale (boundary version = max sequence).
    let before = snap(owner);
    put_j(owner, "p-late-u", "user", "MARK-parent-late-user", max_seq(owner) + 1);
    assert_ne!(snap(owner), before);
    // AUDIT: replay with the SAME intent + SAME (now stale) anchor still succeeds because the
    // stored-intent lookup runs before anchor revalidation.
    assert_eq!(hfork_anchor(&w, owner, anchor.clone(), "same").unwrap(), first);
    // A NEW intent with the stale anchor is rejected by revalidation.
    let err = hfork_anchor(&w, owner, anchor, "fresh").unwrap_err().to_string();
    assert!(err.contains("journey_anchor_changed"), "{err}");

    // Exactly one child + one intent row exists for this owner.
    let c = database::db::get_connection().unwrap();
    let intents: i64 = c
        .query_row("SELECT COUNT(*) FROM historical_runtime_fork_intents WHERE owner_session_id=?1", [owner], |r| r.get(0))
        .unwrap();
    let children: i64 = c
        .query_row("SELECT COUNT(*) FROM agent_sessions WHERE parent_session_id=?1", [owner], |r| r.get(0))
        .unwrap();
    assert_eq!((intents, children), (1, 1));
}

// 5. Child Journey owner-open after runtime removal + restart-style re-registration.
#[test]
fn child_journey_owner_open_after_runtime_removal_and_restart() {
    let _sandbox = test_helpers::test_env::sandbox();
    let owner = "osagent-jfo-reopen";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, owner);
    let (_app, w) = window(&state);
    let child = child_of(&hfork(&w, owner, "f-a1", "reopen-1").unwrap());
    give_child_journey(&child);
    let parent_revision = journey_of(owner).unwrap().revision;
    let child_revision = journey_of(&child).unwrap().revision;
    assert_eq!(parent_revision, 1);
    assert!(child_revision >= 2, "pending task + activation by the first user row");

    // Drop both runtimes (simulated restart): domain reads need a registered owner.
    tauri::async_runtime::block_on(state.remove_session(owner));
    tauri::async_runtime::block_on(state.remove_session(&child));
    assert!(invoke(&w, "journey_domain_snapshot", json!({"ownerSessionId":owner})).is_err());
    assert!(invoke(&w, "journey_domain_snapshot", json!({"ownerSessionId":&child})).is_err());

    // owner_open re-registers each owner and returns an owner-scoped canonical snapshot.
    let ps = invoke(&w, "journey_owner_open", json!({"ownerSessionId":owner})).unwrap();
    let cs = invoke(&w, "journey_owner_open", json!({"ownerSessionId":&child})).unwrap();
    assert_eq!(ps["ownerSessionId"], owner);
    assert_eq!(cs["ownerSessionId"], json!(child));
    assert!(tauri::async_runtime::block_on(state.get_session(owner)).is_some());
    assert!(tauri::async_runtime::block_on(state.get_session(&child)).is_some());
    let ids = |s: &Value, owner: &str| -> Vec<String> {
        let mut v: Vec<String> = s["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                assert_eq!(n["anchor"]["ownerSessionId"], owner);
                assert_eq!(n["anchor"]["sourceSessionId"], owner);
                assert!(n["id"].as_str().unwrap().starts_with(&format!("{owner}:")));
                n["anchor"]["durableMessageId"].as_str().unwrap().to_string()
            })
            .collect();
        v.sort();
        v
    };
    // Canonical nodes are PHYSICAL user rows of that owner only (not Journey-filtered).
    assert_eq!(ids(&ps, owner), strs(&["f-u1", "m-u1"]));
    let mut want_child = vec![format!("{child}:f-u1"), format!("{child}:m-u1"), "child-new-u1".to_string()];
    want_child.sort();
    assert_eq!(ids(&cs, &child), want_child);

    // Domain snapshots after restart agree with the durable aggregates, per owner.
    let pd = invoke(&w, "journey_domain_snapshot", json!({"ownerSessionId":owner})).unwrap();
    let cd = invoke(&w, "journey_domain_snapshot", json!({"ownerSessionId":&child})).unwrap();
    assert_eq!(pd["revision"], json!(parent_revision));
    assert_eq!(cd["revision"], json!(child_revision));

    // Fresh physical SQLite handle: both aggregates survived independently.
    let p = journey_of(owner).unwrap();
    let c = journey_of(&child).unwrap();
    assert_eq!(p.active_branch_id, "f1");
    assert!(p.branches.contains_key("f1") && !p.tasks.contains_key("ct1"));
    assert_eq!(c.active_branch_id, "main");
    assert!(c.tasks.contains_key("ct1") && !c.branches.contains_key("f1") && !c.tasks.contains_key("ft1"));
    assert_eq!(visible_ids(owner), strs(&["m-u1", "f-u1", "f-a1"]));
    assert_eq!(visible_ids(&child), strs(&["child-new-u1", "child-new-a1"]));
}

// 6. Depth-2 fork scoping: P -> C -> G.
#[test]
fn depth2_fork_scoping() {
    let _sandbox = test_helpers::test_env::sandbox();
    let p = "osagent-jfo-depth2";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, p);
    let (_app, w) = window(&state);
    let c = child_of(&hfork(&w, p, "f-a1", "d2-pc").unwrap());
    give_child_journey(&c);
    assert_eq!(visible_ids(&c), strs(&["child-new-u1", "child-new-a1"]));

    // G forks C at C's last row (6 rows: 4 copied + 2 new).
    let g = child_of(&hfork(&w, &c, "child-new-a1", "d2-cg").unwrap());
    assert!(g.starts_with(&format!("{c}-history-")), "{g}");
    assert_eq!(parent_of(&g).as_deref(), Some(c.as_str()));
    assert_eq!(parent_of(&c).as_deref(), Some(p));
    assert_eq!(
        rows(&g).into_iter().map(|r| r.0).collect::<Vec<_>>(),
        rows(&c).into_iter().map(|r| format!("{g}:{}", r.0)).collect::<Vec<_>>()
    );
    // Ids are minted from the DIRECT source's ids: G rows look like `G:C:m-u1`, never `G:m-u1`.
    assert!(rows(&g)[0].0.starts_with(&format!("{g}:{c}:")), "{:?}", rows(&g)[0]);

    // AUDIT: G again has no Journey, so it re-exposes the 4 copied rows that C's own Journey hides.
    assert!(journey_of(&g).is_none());
    assert!(members(&g).is_empty());
    assert_eq!(visible_ids(&g).len(), 6);
    assert_eq!(visible_ids(&c).len(), 2);
    assert!(visible_ids(&g).contains(&format!("{g}:{c}:m-a1")));

    // Intents are scoped per owner: P owns one, C owns one, G none.
    {
        let conn = database::db::get_connection().unwrap();
        let count = |o: &str| -> i64 {
            conn.query_row("SELECT COUNT(*) FROM historical_runtime_fork_intents WHERE owner_session_id=?1", [o], |r| r.get(0))
                .unwrap()
        };
        assert_eq!((count(p), count(&c), count(&g)), (1, 1, 0));
    }

    // Later writes on P and C never reach G.
    let g_before = snap(&g);
    let g_visible = visible_ids(&g);
    put_j(p, "p-late-a", "assistant", "MARK-parent-late", max_seq(p) + 1);
    put_j(&c, "c-late-a", "assistant", "MARK-child-late", max_seq(&c) + 1);
    assert_eq!(snap(&g), g_before);
    assert_eq!(visible_ids(&g), g_visible);
    assert_eq!(visible_ids(&c), strs(&["child-new-u1", "child-new-a1", "c-late-a"]));
    assert_eq!(visible_ids(p), strs(&["m-u1", "f-u1", "f-a1", "p-late-a"]));
}

// 7. Main session and fork child are distinct owners at runtime, in storage and in authority checks.
#[test]
fn main_and_fork_distinct_owners() {
    let _sandbox = test_helpers::test_env::sandbox();
    let p = "osagent-jfo-owners";
    let state = agent_core::state::AgentAppState::new();
    seed_journey_parent(&state, p);
    let (_app, w) = window(&state);
    let c = child_of(&hfork(&w, p, "f-a1", "own-1").unwrap());
    give_child_journey(&c);

    // Distinct runtime objects with independent processing locks.
    let ps = tauri::async_runtime::block_on(state.get_session(p)).unwrap();
    let cs = tauri::async_runtime::block_on(state.get_session(&c)).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&ps, &cs));
    assert_eq!((ps.id.as_str(), cs.id.as_str()), (p, c.as_str()));
    let parent_turn = ps.processing_lock.clone().try_lock_owned().unwrap();
    assert!(cs.processing_lock.clone().try_lock_owned().is_ok(), "busy parent must not busy the child");
    drop(parent_turn);

    // The same logical row has a different canonical identity per owner.
    let pa = anchor_json(p, "f-a1");
    let ca = anchor_json(&c, &format!("{c}:f-a1"));
    assert_ne!(pa["ownerSessionId"], ca["ownerSessionId"]);
    assert_ne!(pa["durableMessageId"], ca["durableMessageId"]);
    assert_ne!(pa["boundaryHash"], ca["boundaryHash"]);
    assert_eq!(pa["sourceSessionId"], pa["ownerSessionId"]);
    assert_eq!(ca["sourceSessionId"], ca["ownerSessionId"]);

    // Independent Journey aggregates.
    let pj = journey_of(p).unwrap();
    let cj = journey_of(&c).unwrap();
    assert_eq!(pj.revision, 1);
    assert_eq!(pj.branches.keys().cloned().collect::<Vec<_>>(), strs(&["f1", "main"]));
    assert_eq!(cj.branches.keys().cloned().collect::<Vec<_>>(), strs(&["main"]));
    assert!(members(p).iter().all(|m| !m.0.starts_with(&c)));
    assert!(members(&c).iter().all(|m| m.0 == "child-new-u1" || m.0 == "child-new-a1"));

    // Cross-owner authority is refused and leaves both aggregates untouched.
    let (ps0, cs0) = (snap(p), snap(&c));
    let err = invoke(&w, "journey_boundary_preview", json!({"ownerSessionId":&c,"anchor":pa})).unwrap_err().to_string();
    assert!(err.contains("journey_source_unauthorized"), "{err}");
    let err = invoke(
        &w,
        "journey_task_start",
        json!({"ownerSessionId":&c,"request":{"sessionId":p,"expectedRevision":1,"taskId":"x","name":"x","position":"下一条用户消息"}}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("journey_owner_mismatch"), "{err}");
    let err = invoke(
        &w,
        "journey_task_start",
        json!({"ownerSessionId":p,"request":{"sessionId":&c,"expectedRevision":1,"taskId":"x","name":"x","position":"下一条用户消息"}}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("journey_owner_mismatch"), "{err}");
    assert_eq!((snap(p), snap(&c)), (ps0, cs0));
}
