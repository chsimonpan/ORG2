use super::*;
use std::collections::HashSet;
use crate::tools::registration::{agent_ops, ToolDeps};
fn deps(owner: &str) -> ToolDeps {
    ToolDeps {
        workspace: Arc::new(parking_lot::RwLock::new(crate::session::SessionWorkspace::new(std::path::PathBuf::from("/org2/fixtures/native")))),
        scratchpad_dir: None, readonly_extra_dirs: vec![], exec_timeout: 10,
        restrict_to_workspace: true, pty_sessions: None, app_handle: None,
        security_policy: None, action_bridge: None, execution_mode: Default::default(),
        agent_browser_config: None, screenshot_store: None, web_search_api_key: None,
        desktop_enabled: false, agent_model: "mock-import".into(), session_id: owner.into(),
        bus: None, session_account_id: None, node_registry: None, question_manager: None,
        secret_broker: None, plan_approval_manager: None, plan_slot_cache: None,
        agent_org_context: None, agent_org_current_member_id: None, channel_context: None,
    }
}
fn registry(owner: &str) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    agent_ops::register(&mut registry, &deps(owner), &HashSet::new());
    registry
}
#[test]
fn native_import_context_is_registered_by_the_real_session_builder_category() {
    let r = registry("import-native-owner");
    assert!(r.has("import_context"), "native import_context missing from production agent_ops registration");
    assert!(r.get_definitions().iter().any(|d| d["function"]["name"] == "import_context"));
    let mut r = ToolRegistry::new();
    agent_ops::register(&mut r, &deps("import-native-owner"), &HashSet::from(["import_context".into()]));
    assert!(!r.has("import_context"), "disabled tool must remain unavailable");
}


use crate::session::journey_canonical::{resolve_with_connection,DurableAnchor};
use rusqlite::params;
fn seed(owner:&str)->DurableAnchor {
    crate::foundation::db_bridge::register(database::db::get_connection);
    let c=database::db::get_connection().unwrap();
    crate::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
    // Hermetic per-owner reset: the DB under HOME persists across test runs, so stale rows
    // (extra imports, a prior run's revision-bumping message) must not leak into this run.
    c.execute("DELETE FROM agent_messages WHERE session_id=?1",[owner]).unwrap();
    if c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='session_context_imports_v1')",[],|r|r.get::<_,bool>(0)).unwrap() {
        c.execute("DELETE FROM session_context_imports_v1 WHERE target_session_id=?1",[owner]).unwrap();
    }
    c.execute("INSERT OR REPLACE INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-08T00:00:00Z','2026-10-08T00:00:00Z')",[owner]).unwrap();
    for (suffix,sequence,role,content) in [("start",1,"user","Important prior decision: imports must be explicit."),("end",2,"assistant","Keep the exact durable boundary."),("future",3,"user","FUTURE_AFTER_SELECTED_END_MUST_NOT_BE_IMPORTED")] {
        c.execute("INSERT OR REPLACE INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?4,?5,'2026-10-08T00:00:00Z')",params![format!("{owner}-{suffix}"),owner,role,content,sequence]).unwrap();
    }
    resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),&format!("{owner}-end"),3).unwrap()
}
fn args(owner:&str,anchor:&DurableAnchor)->Value {serde_json::json!({"source_kind":"session","source_id":owner,"anchor":anchor,"token_budget":2048,"title":"Native Selected Context","pinned":true})}
fn count(owner:&str)->i64 {let c=database::db::get_connection().unwrap();c.query_row("SELECT COUNT(*) FROM session_context_imports_v1 WHERE target_session_id=?1",[owner],|r|r.get(0)).unwrap_or(0)}
struct ImportProvider { owner:String, arguments:Value, calls:AtomicU32 }
#[async_trait]
impl LLMProvider for ImportProvider {
 async fn chat(&self,messages:&[Value],tools:Option<&[Value]>,_model:&str,_max_tokens:u32,_temperature:f32)->Result<LLMResponse,ProviderError>{
    let call=self.calls.fetch_add(1,Ordering::SeqCst);
    let tool_calls=if call==0 {
        assert!(tools.unwrap().iter().any(|d|d["function"]["name"]=="import_context"),"real native provider schema missing registration");
        vec![ToolCallRequest{id:"native-import-call".into(),name:"import_context".into(),arguments:self.arguments.clone(),thought_signature:None}]
    }else{
        let result=messages.iter().find(|m|m["role"]=="tool"&&m["tool_call_id"]=="native-import-call").expect("real turn dispatcher did not supply tool result");
        let raw=result["content"].as_str().expect("tool text");
        let snapshot:crate::session::context_import::ContextSnapshot=serde_json::from_str(raw).expect("native tool result is not the durable card contract");
        assert_eq!(snapshot.target_session_id,self.owner);assert_eq!(snapshot.source_id,self.owner);
        assert!(snapshot.pinned);assert!(snapshot.snippet.contains("imports must be explicit"));
        assert!(!snapshot.snippet.contains("FUTURE_AFTER_SELECTED_END"));
        vec![]
    };
    Ok(LLMResponse{content:if call==0{None}else{Some("native import persisted".into())},finish_reason:if call==0{finish_reason::TOOL_CALLS.into()}else{finish_reason::STOP.into()},tool_calls,usage:HashMap::new(),reasoning_content:None,blocks:vec![],stream_error_kind:None,retry_after_ms:None})
 }
 fn default_model(&self)->&str{"mock-import"}
 fn provider_name(&self)->&str{"private-test-provider"}
}
#[tokio::test]
async fn native_import_context_real_provider_turn_persists_and_hydrates_after_reopen() {
    let owner="import-e2e-owner";let anchor=seed(owner);let r=registry(owner);
    let provider=ImportProvider{owner:owner.into(),arguments:args(owner,&anchor),calls:AtomicU32::new(0)};
    let mut messages=vec![serde_json::json!({"role":"user","content":"Import the explicitly selected boundary"})];
    let result=execute_turn(&mut messages,&provider,&r,&empty_policy(),&test_config(),owner,&MockRetryHandler::new(),None,None,None).await.unwrap();
    assert_eq!(result.content.as_deref(),Some("native import persisted"));assert_eq!(provider.calls.load(Ordering::SeqCst),2);assert_eq!(count(owner),1);
    // Open a fresh independent SQLite connection, not an in-memory fake store.
    let c=rusqlite::Connection::open(database::db::get_db_path()).unwrap();
    let sections=crate::session::context_import::provider_sections_with_connection(&c,owner).unwrap();assert_eq!(sections.len(),1);
    let snippet=sections[0]["content"].as_str().unwrap();assert!(snippet.contains("# Imported Context"));assert!(snippet.contains("session:import-e2e-owner"));assert!(!snippet.contains("FUTURE_AFTER_SELECTED_END"));
    assert!(crate::session::context_import::provider_sections_with_connection(&c,"not-this-owner").unwrap().is_empty());drop(c);
    // The production provider reconstruction path must actually include the snapshot.
    let restored=crate::foundation::persistence::db_helpers::load_llm_history("agent",owner).unwrap();
    assert!(restored.iter().any(|m|m["role"]=="system"&&m["content"].as_str().is_some_and(|s|s.contains("# Imported Context"))));
}
#[tokio::test]
async fn native_import_context_rejects_untrusted_cross_owner_budget_and_stale_anchor_without_writes(){
    let owner="import-guard-owner";let anchor=seed(owner);let r=registry(owner);let tool=r.get("import_context").unwrap();
    let context=CallContext{session_id:owner.into(),call_id:"guard-call".into(),..CallContext::trusted_sde()};
    let p=args(owner,&anchor);
    assert!(tool.execute_text(p.clone(),&CallContext::new("untrusted",owner)).await.is_err());
    assert!(tool.execute_text(p.clone(),&CallContext{session_id:"other".into(),..CallContext::trusted_sde()}).await.is_err());
    let mut foreign=p.clone();foreign["source_id"]=Value::String("other".into());assert!(tool.execute_text(foreign,&context).await.is_err());
    let mut small=p.clone();small["token_budget"]=Value::from(1);assert!(tool.execute_text(small,&context).await.unwrap_err().to_string().contains("budget_exceeded"));
    let mut spoof=p.clone();spoof["snippet"]=Value::String("LLM-fabricated bytes".into());assert!(tool.execute_text(spoof,&context).await.is_err());
    let mut wrong=p.clone();wrong["anchor"]["branchId"]=Value::String("topic-label".into());assert!(tool.execute_text(wrong,&context).await.is_err());
    let mut wrong=p.clone();wrong["anchor"]["durableMessageId"]=Value::String("missing".into());assert!(tool.execute_text(wrong,&context).await.is_err());
    let c=database::db::get_connection().unwrap();c.execute("UPDATE agent_messages SET content='changed at same durable ID' WHERE id=?1",[format!("{owner}-end")]).unwrap();drop(c);
    assert!(tool.execute_text(p,&context).await.unwrap_err().to_string().contains("anchor_changed"));assert_eq!(count(owner),0);
    let anchor=seed(owner);let p=args(owner,&anchor);let c=database::db::get_connection().unwrap();c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,'user','new revision',4,'2026-10-08T00:00:00Z')",params![format!("{owner}-new"),owner]).unwrap();drop(c);
    assert!(tool.execute_text(p,&context).await.unwrap_err().to_string().contains("revision_changed"));assert_eq!(count(owner),0);
}
