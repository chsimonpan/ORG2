//! Real shared production coding registration; no simulated tool implementations.
use std::{collections::HashSet, sync::Arc};
use serde_json::json;
use crate::{tools::{registration::{ToolDeps, coding}, registry::ToolRegistry, call_context::CallContext}, session::workspace::SessionWorkspace, security::SecurityPolicy};
use crate::foundation::security::global_paths::{global_path_exemptions_add as add, global_path_exemptions_remove as remove, global_path_exemptions_list as list};

#[tokio::test]
async fn persisted_grants_real_registered_tools_revoke_without_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    let outside = temp.path().join("outside");
    let home = temp.path().join("home");
    for p in [&root, &outside, &home] { std::fs::create_dir_all(p).unwrap(); }
    // Dedicated single-test process: management service uses its real persisted store.
    std::env::set_var("ORGII_HOME", &home);
    let file = outside.join("fixture.rs");
    std::fs::write(&file, "fn matrix_marker() {}\n").unwrap();
    let mut policy = SecurityPolicy::permissive();
    policy.workspace_only = true;
    policy.forbidden_paths = vec![home.to_string_lossy().into_owned()];
    policy.blocked_commands = vec!["rm".into()];
    let deps = ToolDeps {
        workspace: Arc::new(parking_lot::RwLock::new(SessionWorkspace::new(root.clone()))),
        scratchpad_dir: None, readonly_extra_dirs: vec![], exec_timeout: 10,
        restrict_to_workspace: true, pty_sessions: None, app_handle: None,
        security_policy: Some(Arc::new(policy)), action_bridge: None,
        execution_mode: crate::integrations::config::ExecutionMode::Direct,
        agent_browser_config: None, screenshot_store: None, web_search_api_key: None,
        desktop_enabled: false, agent_model: String::new(), session_id: "matrix".into(),
        bus: None, session_account_id: None, node_registry: None, question_manager: None,
        secret_broker: None, plan_approval_manager: None, plan_slot_cache: None,
        agent_org_context: None, agent_org_current_member_id: None, channel_context: None,
    };
    let mut registry = ToolRegistry::new();
    coding::register(&mut registry, &deps, &HashSet::new());
    let mut context = CallContext::trusted_sde();
    context.session_id = "matrix".into();
    context.call_id = "matrix-exec-1".into();
    let read = registry.get("read_file").unwrap();
    let edit = registry.get("edit_file").unwrap();
    let search = registry.get("code_search").unwrap();
    let exec = registry.get(crate::tools::names::RUN_SHELL).unwrap();
    let read_args = json!({"path": file});
    let edit_args = json!({"file_path": file, "content": "fn updated_marker() {}\n"});
    let search_args = json!({"action":"find_files", "repo_path": outside, "pattern":"*.rs", "max_results":10});
    let exec_args = json!({"command":"printf matrix_exec", "working_dir":outside});
    assert!(read.execute(read_args.clone(), &context).await.is_err());
    assert!(search.execute(search_args.clone(), &context).await.is_err());
    assert!(exec.execute(exec_args.clone(), &context).await.is_err());
    add(outside.to_string_lossy().into(), "read".into(), true).await.unwrap();
    assert_eq!(list().await.unwrap().len(), 1);
    assert!(read.execute(read_args.clone(), &context).await.is_ok());
    assert!(search.execute(search_args.clone(), &context).await.is_ok());
    assert!(edit.execute(edit_args.clone(), &context).await.is_err());
    assert!(exec.execute(exec_args.clone(), &context).await.is_err());
    add(outside.to_string_lossy().into(), "read_write".into(), true).await.unwrap();
    assert!(edit.execute(edit_args.clone(), &context).await.is_ok());
    assert!(std::fs::read_to_string(&file).unwrap().contains("updated_marker"));
    let execution = exec.execute(exec_args.clone(), &context).await;
    assert!(execution.is_ok(), "real exec failed: {execution:?}");
    #[cfg(unix)] {
        let escape = root.join("escape");
        std::os::unix::fs::symlink(temp.path().join("home"), &escape).unwrap();
        std::fs::write(home.join("private.txt"), "private").unwrap();
        add(home.to_string_lossy().into(), "read_write".into(), true).await.unwrap();
        assert!(read.execute(json!({"path":escape.join("private.txt")}), &context).await.is_err());
    }
    remove(home.to_string_lossy().into()).await.unwrap();
    remove(outside.to_string_lossy().into()).await.unwrap();
    assert!(list().await.unwrap().is_empty());
    assert!(read.execute(read_args, &context).await.is_err());
    assert!(search.execute(search_args, &context).await.is_err());
    assert!(edit.execute(edit_args, &context).await.is_err());
    assert!(exec.execute(exec_args, &context).await.is_err());
}
