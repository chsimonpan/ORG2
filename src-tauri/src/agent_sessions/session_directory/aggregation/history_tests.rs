use super::*;

#[test]
fn pinned_history_uses_the_same_identity_and_excludes_ordinary_copies_before_limit() {
    let _sandbox = crate::test_utils::test_env::sandbox();
    let conn = get_connection().unwrap();
    conn.execute_batch(
        "INSERT INTO agent_sessions
        (session_id,name,status,session_type,created_at,updated_at,org_member_id,pinned)
        VALUES ('sdeagent-retired','Preserved team','archived','sde','now','now','coordinator',1),
               ('sdeagent-copy','Copy','idle','sde','now','newer',NULL,1);
        INSERT INTO org_history_sessions(session_id,root_session_id,title,source_table)
        VALUES ('sdeagent-retired','sdeagent-retired','Preserved team','fixture');
        INSERT INTO org_history_copies VALUES ('sdeagent-retired','sdeagent-copy');",
    )
    .unwrap();
    let rows = list_pinned_native_sidebar_sessions(1, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id, "sdeagent-retired");
    assert_eq!(
        rows[0].agent_org_mode,
        Some(agent_core::coordination::agent_org_history_store::HistoryMode::HistoryOnly)
    );
    assert!(
        rows[0].agent_org_id.is_none(),
        "history must not invent an operational team identity"
    );
}

#[test]
fn pinned_durable_child_keeps_parent_and_excludes_delegates_and_foreign_intents() {
    let _sandbox = crate::test_utils::test_env::sandbox();
    let conn = get_connection().unwrap();
    agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO agent_sessions
        (session_id,name,status,session_type,created_at,updated_at,parent_session_id,pinned)
        VALUES ('sdeagent-historical','Child','idle','sde','now','c','owner',1),
               ('sdeagent-delegate','Delegate','idle','sde','now','z','owner',1),
               ('sdeagent-foreign','Foreign','idle','sde','now','y','owner',1),
               ('sdeagent-owner','Owner','idle','sde','now','a',NULL,1);
        INSERT INTO historical_runtime_fork_intents VALUES
               ('owner','good','{}','sdeagent-historical','owner'),
               ('other','bad','{}','sdeagent-foreign','owner');"
    ).unwrap();
    drop(conn);
    let first = list_pinned_native_sidebar_sessions(1, None).unwrap();
    assert_eq!(first[0].session_id, "sdeagent-historical");
    let cursor = NativeSidebarSessionCursor { updated_at: "c".into(), session_id: first[0].session_id.clone() };
    let second = list_pinned_native_sidebar_sessions(1, Some(&cursor)).unwrap();
    assert_eq!(second[0].session_id, "sdeagent-owner");
}
