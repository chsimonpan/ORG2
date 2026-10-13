//! Real native dispatcher registered-session roundtrip. No fake HTML owner or generated transcript.
use super::*;
use serde_json::{Value,json};
use tauri::test::{mock_builder,mock_context,noop_assets,MockRuntime};
fn invoke(w:&tauri::WebviewWindow<MockRuntime>,cmd:&str,body:Value)->Result<Value,Value>{
 tauri::test::get_ipc_response(w,tauri::webview::InvokeRequest{cmd:cmd.into(),callback:tauri::ipc::CallbackFn(0),error:tauri::ipc::CallbackFn(1),url:w.url().unwrap(),body:tauri::ipc::InvokeBody::Json(body),headers:Default::default(),invoke_key:tauri::test::INVOKE_KEY.into()}).map(|b|b.deserialize::<Value>().unwrap())
}
/// `KEY_SERVICE` is a process-wide `LazyLock` bound to the first sandbox that touched it; that sandbox's
/// directory is deleted when its guard drops, so a later `save_key` in the same process fails with
/// "Failed to write credentials temp file". Recreate the directory (never delete it: when it is the live
/// sandbox root it also holds the test DB) and keep `credentials.json` empty before and after each test.
/// Refuses any directory outside the system temp dir so it can never touch a real `~/.orgii`.
struct KeyVaultFixture(std::path::PathBuf);
fn clear_credentials(dir:&std::path::Path){for f in ["credentials.json","credentials.json.tmp"]{let _=std::fs::remove_file(dir.join(f));}}
fn key_vault_fixture()->KeyVaultFixture{
 let dir=key_vault::key_store::KEY_SERVICE.get_storage_dir().clone();
 assert!(dir.starts_with(std::env::temp_dir()),"key vault dir {dir:?} is outside the temp dir; refusing to touch it");
 std::fs::create_dir_all(&dir).unwrap();clear_credentials(&dir);KeyVaultFixture(dir)
}
impl Drop for KeyVaultFixture{fn drop(&mut self){clear_credentials(&self.0);}}
#[test]
fn journey_registered_selected_session_durable_task_checkpoint_fork_cas_roundtrip(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="native-selected-journey-owner";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-08T00:00:00Z','2026-10-08T00:00:00Z')",[owner]).unwrap();
 c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('journey-owner-first',?1,'user','Real selected session root',1,'2026-10-08T00:00:00Z')",[owner]).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![journey_snapshot,journey_boundary_preview,journey_domain_snapshot,journey_task_start,journey_task_finish,journey_checkpoint,journey_fork_start,journey_entry_activate]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 assert!(invoke(&w,"journey_snapshot",json!({"ownerSessionId":"cli:foreign"})).is_err());
 let snapshot=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();assert_eq!(snapshot["ownerSessionId"],owner);assert_eq!(snapshot["nodes"][0]["anchor"]["durableMessageId"],"journey-owner-first");
 let domain=invoke(&w,"journey_domain_snapshot",json!({"ownerSessionId":owner})).unwrap();assert_eq!(domain["revision"],0);
 let start=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":0,"taskId":"root-task","name":"Actual native selected task","position":"最近用户消息"}});
 let result=invoke(&w,"journey_task_start",start.clone()).unwrap();assert_eq!(result["revision"],1);
 assert!(invoke(&w,"journey_task_start",start).is_err(),"stale CAS cannot overwrite active task");
 // The endpoint row goes through the actual transcript membership assignment, not a UI label.
 let c=database::db::get_connection().unwrap();let tx=c.unchecked_transaction().unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"journey-owner-first",1,"user").unwrap();
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('journey-owner-selected-end',?1,'user','Exact end of active real task',2,'2026-10-08T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"journey-owner-selected-end",2,"user").unwrap();tx.commit().unwrap();drop(c);
 // A later parent row must remain browsable but never enter selected-end replay.
 let c=database::db::get_connection().unwrap();let tx=c.unchecked_transaction().unwrap();
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('journey-parent-after-end',?1,'assistant','EXCLUDED parent suffix',3,'2026-10-08T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"journey-parent-after-end",3,"assistant").unwrap();tx.commit().unwrap();drop(c);
 let snapshot=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();let node=snapshot["nodes"].as_array().unwrap().iter().find(|n|n["anchor"]["durableMessageId"]=="journey-owner-selected-end").unwrap();let anchor=node["anchor"].clone();assert_eq!(anchor["boundaryVersion"],3);
 let preview=invoke(&w,"journey_boundary_preview",json!({"ownerSessionId":owner,"anchor":anchor})).unwrap();assert_eq!(preview["records"].as_array().unwrap().len(),2);
 let checkpoint=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":1,"checkpointId":"checkpoint-native","name":"Exact checkpoint","messageId":"journey-owner-selected-end"}});
 let result=invoke(&w,"journey_checkpoint",checkpoint.clone()).unwrap();assert_eq!(result["revision"],2);assert!(invoke(&w,"journey_checkpoint",checkpoint).is_err());
 // Historical start_fork contract forbids a second active task; preserve that guard.
 let blocked_fork=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":2,"forkId":"blocked-active-fork","taskId":"blocked-active-task","taskName":"must not mutate","anchorMessageId":"journey-owner-selected-end"},"anchor":anchor});
 assert!(invoke(&w,"journey_fork_start",blocked_fork).is_err());
 let unchanged=invoke(&w,"journey_domain_snapshot",json!({"ownerSessionId":owner})).unwrap();assert_eq!(unchanged["revision"],2);
 let finish=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":2,"outcome":"completed","messageId":"journey-owner-selected-end"}});
 let finished=invoke(&w,"journey_task_finish",finish.clone()).unwrap();assert_eq!(finished["revision"],3);assert!(invoke(&w,"journey_task_finish",finish).is_err());
 let fork=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":3,"forkId":"branch-native","taskId":"fork-task-native","taskName":"Forked exact selected task","anchorMessageId":"journey-owner-selected-end"},"anchor":anchor});
 let result=invoke(&w,"journey_fork_start",fork.clone()).unwrap();assert_eq!(result["revision"],4);assert!(invoke(&w,"journey_fork_start",fork).is_err());
 // Fresh physical connection proves durable branch identity and parent endpoint survived.
 let c=rusqlite::Connection::open(database::db::get_db_path()).unwrap();let journey=agent_core::session::journey_lifecycle::SqliteJourneyRepository::load(&c,owner).unwrap().unwrap();assert_eq!(journey.revision,4);assert_eq!(journey.active_branch_id,"branch-native");assert_eq!(journey.branches["branch-native"].parent_anchor_message_id.as_deref(),Some("journey-owner-selected-end"));drop(c);
 // Persist the child through the same writer membership hook used by real messages.
 let c=database::db::get_connection().unwrap();let tx=c.unchecked_transaction().unwrap();
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('journey-child-replay',?1,'assistant','New child transcript',4,'2026-10-08T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"journey-child-replay",4,"assistant").unwrap();tx.commit().unwrap();drop(c);
 for _ in 0..2 {
  let c=rusqlite::Connection::open(database::db::get_db_path()).unwrap();
  let rows=agent_core::foundation::persistence::db_helpers::load_messages("agent",owner).unwrap();assert_eq!(rows.len(),4);
  let (visible,capsules)=agent_core::session::journey_runtime_persistence::provider_visible_rows(&c,owner,rows).unwrap();
  assert_eq!(visible.iter().map(|r|r.id.as_str()).collect::<Vec<_>>(),vec!["journey-owner-first","journey-owner-selected-end","journey-child-replay"]);assert!(capsules.is_empty());
  assert_eq!(c.query_row("SELECT branch_id FROM session_journey_memberships WHERE session_id=?1 AND message_id='journey-child-replay'",[owner],|r|r.get::<_,String>(0)).unwrap(),"branch-native");
 }
 let activate=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":4,"taskId":"root-task","branchId":"main"}});
 let result=invoke(&w,"journey_entry_activate",activate).unwrap();assert_eq!(result["revision"],5);
 let session=tauri::async_runtime::block_on(state.get_session(owner)).unwrap();let lease=session.processing_lock.clone().try_lock_owned().unwrap();
 assert!(invoke(&w,"journey_task_start",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":5,"taskId":"busy-task","name":"denied","position":"下一条用户消息"}})).is_err());drop(lease);
 tauri::async_runtime::block_on(state.remove_session(owner));assert!(invoke(&w,"journey_domain_snapshot",json!({"ownerSessionId":owner})).is_err());
}

#[test]
fn journey_registered_canvas_refresh_invalidates_old_native_endpoint(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="native-canvas-refresh-owner";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-08T00:00:00Z','2026-10-08T00:00:00Z')",[owner]).unwrap();
 c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('canvas-refresh-end',?1,'user','Before durable edit',1,'2026-10-08T00:00:00Z')",[owner]).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![journey_snapshot,journey_boundary_preview]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let before=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();let anchor=before["nodes"][0]["anchor"].clone();
 assert!(invoke(&w,"journey_boundary_preview",json!({"ownerSessionId":owner,"anchor":anchor})).is_ok());
 let c=database::db::get_connection().unwrap();c.execute("UPDATE agent_messages SET content='After durable edit' WHERE id='canvas-refresh-end'",[]).unwrap();drop(c);
 assert!(invoke(&w,"journey_boundary_preview",json!({"ownerSessionId":owner,"anchor":anchor})).is_err(),"old selected endpoint must not authorize edited source");
 let after=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();assert_ne!(before["nodes"][0]["anchor"]["boundaryHash"],after["nodes"][0]["anchor"]["boundaryHash"]);
 assert!(invoke(&w,"journey_boundary_preview",json!({"ownerSessionId":owner,"anchor":after["nodes"][0]["anchor"]})).is_ok());
 tauri::async_runtime::block_on(state.remove_session(owner));assert!(invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).is_err());
}

#[test]
fn journey_explicit_durable_owner_open_rehydrates_without_creating_or_model_work(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="sdeagent-journey-reopen-fixture";let state=agent_core::state::AgentAppState::new();
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![journey_owner_open,journey_snapshot]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 assert!(invoke(&w,"journey_owner_open",json!({"ownerSessionId":owner})).is_err());assert!(tauri::async_runtime::block_on(state.get_session(owner)).is_none());
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-08T00:00:00Z','2026-10-08T00:00:00Z')",[owner]).unwrap();drop(c);
 assert!(invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).is_err());
 let result=invoke(&w,"journey_owner_open",json!({"ownerSessionId":owner})).unwrap();assert_eq!(result["ownerSessionId"],owner);assert!(tauri::async_runtime::block_on(state.get_session(owner)).is_some());assert!(invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).is_ok());
 assert!(invoke(&w,"journey_owner_open",json!({"ownerSessionId":"cli:foreign"})).is_err());
}

#[test]
fn rerank_registered_config_dispatch_persistent_provider_error_contract() {
 let _sandbox=test_helpers::test_env::sandbox();
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let state=agent_core::state::AgentAppState::new();
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![agent_core::definitions::commands::integrations_get,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 assert!(invoke(&w,"integrations_update_patch",json!({"rerank":{"provider":"disabled"}})).is_err(),"missing patch envelope must reject");
 assert!(invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"local","requestTimeoutSecs":"bad"}}})).is_err());
 let before=invoke(&w,"integrations_get",json!({})).unwrap();
 let updated=invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled","model":null,"baseUrl":null,"requestTimeoutSecs":19}}})).unwrap();
 assert_eq!(updated["rerank"]["requestTimeoutSecs"],19);
 assert_eq!(updated["embedding"],before["embedding"],"unrelated configuration preserved");
 let loaded=agent_core::state::AgentAppState::new();
 assert_eq!(serde_json::to_value(loaded.integrations.snapshot()).unwrap()["rerank"],updated["rerank"],"disk reload preserves native patch");
 let c=state.integrations.snapshot().rerank;
 assert!(agent_core::specialization::memory::embeddings::rerank::ConfiguredReranker::from_config(c).unwrap().is_disabled());
 let unknown=invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"unsupported-explicit","model":null,"baseUrl":null,"requestTimeoutSecs":19}}})).unwrap();
 assert_eq!(unknown["rerank"]["provider"],"unsupported-explicit");
 assert!(agent_core::specialization::memory::embeddings::rerank::ConfiguredReranker::from_config(state.integrations.snapshot().rerank).err().unwrap().contains("unsupported"));
 assert!(invoke(&w,"rerank",json!({"ownerSessionId":"foreign","query":"q","documents":["d"]})).is_err(),"no registered rerank execution command; do not claim execution chain");
}

#[test]
fn memory_search_registered_execution_contract(){
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 let state=agent_core::state::AgentAppState::new(); let owner="memory-owner";
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::core::session::journey_embedding::ensure_schema(&c).unwrap();
 let mut v=vec![0f32;1024];v[0]=1.;let blob:Vec<u8>=v.iter().flat_map(|x|x.to_le_bytes()).collect();
 let source="openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024";
 for (sid,id,text) in [(owner,"a","alpha"),(owner,"b","beta"),("foreign","c","secret")]{c.execute("INSERT INTO session_journey_embedding_index(source_id,source_kind,session_id,fork_id,review_id,content,embedding,embedding_model,embedding_source,embedding_dimensions) VALUES(?1,'confirmed_fact',?2,'f','r',?3,?4,'qwen/qwen3-vl-embedding',?5,1024)",rusqlite::params![id,sid,text,blob,source]).unwrap();}drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![crate::memory_search_commands::memory_search,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled"}}})).unwrap();
 let body=json!({"ownerSessionId":owner,"query":"q","queryVector":v,"embeddingSource":source,"topN":2});
 let hits=invoke(&w,"memory_search",body.clone()).unwrap();assert_eq!(hits.as_array().unwrap().len(),2);assert_eq!(hits[0]["sourceId"],"a");assert_eq!(hits[1]["sourceId"],"b");
 let mut wrong=body.clone();wrong["queryVector"]=json!([1.0]);assert!(invoke(&w,"memory_search",wrong).is_err());
 let mut foreign=body.clone();foreign["ownerSessionId"]=json!("foreign");assert!(invoke(&w,"memory_search",foreign).is_err());
 let mut old=body.clone();old["ownerSessionId"]=json!("cli:old");assert!(invoke(&w,"memory_search",old).is_err());
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"unsupported-explicit"}}})).unwrap();assert!(invoke(&w,"memory_search",body.clone()).is_err());
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled"}}})).unwrap();assert!(invoke(&w,"memory_search",body.clone()).is_ok());
 let c=database::db::get_connection().unwrap();c.execute("UPDATE session_journey_embedding_index SET embedding_dimensions=1536 WHERE session_id=?1",[owner]).unwrap();drop(c);assert!(invoke(&w,"memory_search",body).is_err());
}

#[test]
fn memory_query_transport_registered_contract(){
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 let _kv=key_vault_fixture();
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path,body_json};
 let server=MockServer::start().await;let mut vector=vec![0.0f32;1024];vector[0]=1.;
 Mock::given(method("POST")).and(path("/api/v1/embeddings")).and(body_json(json!({"model":"qwen/qwen3-vl-embedding","input":"transport query","encoding_format":"float","dimensions":1024}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":vector}],"model":"qwen/qwen3-vl-embedding","usage":{"prompt_tokens":30,"total_tokens":30,"cost":"0.000003","cost_details":{"prompt":"0.000003","completion":"0"}}}))).expect(1).mount(&server).await;
 let mut key=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::OpenaiApi);key.api_key=Some("synthetic-no-production-key".into());key.base_url=Some(format!("{}/api/v1",server.uri()));key.enabled_models=vec!["qwen/qwen3-vl-embedding".into()];key.health_status=key_vault::key_store::HealthStatus::Valid;key.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(key).unwrap();
 let state=agent_core::state::AgentAppState::new();let owner="query-fixture-owner";state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())).await;
 let c=database::db::get_connection().unwrap();agent_core::core::session::journey_embedding::ensure_schema(&c).unwrap();let blob:Vec<u8>=vector.iter().flat_map(|v|v.to_le_bytes()).collect();
 c.execute("INSERT INTO session_journey_embedding_index(source_id,source_kind,session_id,fork_id,review_id,content,embedding,embedding_model,embedding_source,embedding_dimensions) VALUES('query-fixture','confirmed_fact',?1,'f','r','synthetic content',?2,'qwen/qwen3-vl-embedding','openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024',1024)",rusqlite::params![owner,blob]).unwrap();drop(c);
 let app=mock_builder().manage(state).invoke_handler(tauri::generate_handler![crate::memory_search_commands::memory_search,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1024},"rerank":{"provider":"disabled"}}})).unwrap();
 let result=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).unwrap();assert_eq!(result[0]["sourceId"],"query-fixture");
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1536}}})).unwrap();assert!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).is_err());
 server.verify().await;
 });
}

#[test]
fn memory_full_embedding_rerank_registered_contract(){
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 let _kv=key_vault_fixture();
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path,body_json};
 let server=MockServer::start().await;let mut vector=vec![0.0f32;1024];vector[0]=1.;
 Mock::given(method("POST")).and(path("/api/v1/embeddings")).and(body_json(json!({"model":"qwen/qwen3-vl-embedding","input":"transport query","encoding_format":"float","dimensions":1024}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":vector}],"model":"qwen/qwen3-vl-embedding","usage":{"prompt_tokens":30,"total_tokens":30,"cost":"0.000003","cost_details":{"prompt":"0.000003","completion":"0"}}}))).expect(4).mount(&server).await;
 let mut key=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::OpenaiApi);key.api_key=Some("synthetic-no-production-key".into());key.base_url=Some(format!("{}/api/v1",server.uri()));key.enabled_models=vec!["qwen/qwen3-vl-embedding".into(),"qwen/qwen3-vl-rerank".into()];key.health_status=key_vault::key_store::HealthStatus::Valid;key.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(key).unwrap();
 let state=agent_core::state::AgentAppState::new();let owner="query-fixture-owner";state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())).await;
 let c=database::db::get_connection().unwrap();agent_core::core::session::journey_embedding::ensure_schema(&c).unwrap();let blob:Vec<u8>=vector.iter().flat_map(|v|v.to_le_bytes()).collect();
 c.execute("INSERT INTO session_journey_embedding_index(source_id,source_kind,session_id,fork_id,review_id,content,embedding,embedding_model,embedding_source,embedding_dimensions) VALUES('query-fixture','confirmed_fact',?1,'f','r','synthetic content',?2,'qwen/qwen3-vl-embedding','openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024',1024)",rusqlite::params![owner,blob]).unwrap();drop(c);
 let app=mock_builder().manage(state).invoke_handler(tauri::generate_handler![crate::memory_search_commands::memory_search,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1024},"rerank":{"provider":"disabled"}}})).unwrap();
 let result=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).unwrap();assert_eq!(result[0]["sourceId"],"query-fixture");
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1536}}})).unwrap();assert!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).is_err());

 let mut rerank_key=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::ZenmuxApi);rerank_key.api_key=Some("synthetic-zenmux-no-production-key".into());rerank_key.base_url=Some(format!("{}/api/v1",server.uri()));rerank_key.enabled=true;rerank_key.enabled_models=vec!["qwen/qwen3-vl-rerank".into()];rerank_key.health_status=key_vault::key_store::HealthStatus::Valid;rerank_key.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(rerank_key).unwrap();
 Mock::given(method("POST")).and(path("/api/v1/rerank")).and(body_json(json!({"model":"qwen/qwen3-vl-rerank","input":{"query":"transport query","documents":["synthetic content"]},"parameters":{"top_n":1,"return_documents":true}}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[{"index":0,"relevance_score":0.25}]}))).expect(1).mount(&server).await;
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1024},"rerank":{"provider":"zenmux_api","model":"qwen/qwen3-vl-rerank","baseUrl":format!("{}/api/v1",server.uri())}}})).unwrap();
 let result=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).unwrap();assert_eq!(result[0]["sourceId"],"query-fixture");assert_eq!(result[0]["score"],0.25);
 // Every request observes the persisted native configuration patch; transport errors never return cosine hits.
 Mock::given(method("POST")).and(path("/api/v1/rerank")).respond_with(ResponseTemplate::new(503)).with_priority(1).expect(1).mount(&server).await;
 assert!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).is_err());
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled"}}})).unwrap();
 assert_eq!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","topN":10})).unwrap()[0]["score"],1.0);
 let c=database::db::get_connection().unwrap();let mut invalid=vec![0.0f32;1024];invalid[0]=f32::NAN;let blob:Vec<u8>=invalid.iter().flat_map(|x|x.to_le_bytes()).collect();c.execute("UPDATE session_journey_embedding_index SET embedding=?1 WHERE session_id=?2",rusqlite::params![blob,owner]).unwrap();drop(c);
 let mut valid=vec![0.0f32;1024];valid[0]=1.;assert!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"transport query","queryVector":valid,"embeddingSource":"openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024","topN":10})).is_err());
 server.verify().await;
 });
}

#[test]
fn memory_search_candidate_hard_caps_registered_contract(){
 // Hard limits: the index scan is bounded (explicit error above it) and only the cosine top-K reach the rerank provider.
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 let _kv=key_vault_fixture();
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path};use std::sync::{Arc,Mutex};
 use crate::memory_search_commands::{MAX_CANDIDATES,MAX_RERANK_CANDIDATES,SOURCE};
 let server=MockServer::start().await;let seen:Arc<Mutex<Vec<Value>>>=Arc::new(Mutex::new(Vec::new()));let log=seen.clone();
 Mock::given(method("POST")).and(path("/api/v1/rerank")).respond_with(move|req:&wiremock::Request|{let b:Value=serde_json::from_slice(&req.body).unwrap();log.lock().unwrap().push(b);ResponseTemplate::new(200).set_body_json(json!({"results":[{"index":MAX_RERANK_CANDIDATES-1,"relevance_score":0.9},{"index":0,"relevance_score":0.1}]}))}).mount(&server).await;
 let mut rk=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::ZenmuxApi);rk.api_key=Some("synthetic-zenmux-no-production-key".into());rk.base_url=Some(format!("{}/api/v1",server.uri()));rk.enabled=true;rk.enabled_models=vec!["qwen/qwen3-vl-rerank".into()];rk.health_status=key_vault::key_store::HealthStatus::Valid;rk.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(rk).unwrap();
 let state=agent_core::state::AgentAppState::new();let owner="cap-owner";
 state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())).await;
 let c=database::db::get_connection().unwrap();agent_core::core::session::journey_embedding::ensure_schema(&c).unwrap();
 // Row i has cosine to the query strictly decreasing in i.
 let blob=|i:usize|{let mut v=vec![0f32;1024];v[0]=1.;v[1]=i as f32/1000.;v.iter().flat_map(|x|x.to_le_bytes()).collect::<Vec<u8>>()};
 let insert=|c:&rusqlite::Connection,sid:&str,range:std::ops::Range<usize>|{let tx=c.unchecked_transaction().unwrap();for i in range{tx.execute("INSERT INTO session_journey_embedding_index(source_id,source_kind,session_id,fork_id,review_id,content,embedding,embedding_model,embedding_source,embedding_dimensions) VALUES(?1,'confirmed_fact',?2,'f','r',?3,?4,'qwen/qwen3-vl-embedding',?5,1024)",rusqlite::params![format!("s{i:04}"),sid,format!("doc-{i:04}"),blob(i),SOURCE]).unwrap();}tx.commit().unwrap();};
 insert(&c,owner,0..MAX_RERANK_CANDIDATES+20);insert(&c,"foreign-owner",0..3);drop(c);
 let app=mock_builder().manage(state).invoke_handler(tauri::generate_handler![crate::memory_search_commands::memory_search,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let mut q=vec![0f32;1024];q[0]=1.;let body=json!({"ownerSessionId":owner,"query":"capq","queryVector":q,"embeddingSource":SOURCE,"topN":2});
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"zenmux_api","model":"qwen/qwen3-vl-rerank","baseUrl":format!("{}/api/v1",server.uri())}}})).unwrap();
 let hits=invoke(&w,"memory_search",body.clone()).unwrap();let hits=hits.as_array().unwrap();
 { let seen=seen.lock().unwrap();assert_eq!(seen.len(),1);let docs=seen[0]["input"]["documents"].as_array().unwrap();
   assert_eq!(docs.len(),MAX_RERANK_CANDIDATES,"rerank provider gets at most the cap, not every candidate");
   assert_eq!((docs[0].as_str(),docs[MAX_RERANK_CANDIDATES-1].as_str()),(Some("doc-0000"),Some(format!("doc-{:04}",MAX_RERANK_CANDIDATES-1).as_str())),"cosine top-K in cosine order");
   assert!(docs.iter().all(|d|d.as_str().unwrap().starts_with("doc-")));assert_eq!(seen[0]["parameters"]["top_n"],2);}
 assert_eq!(hits.len(),2);assert_eq!(hits[0]["sourceId"],format!("s{:04}",MAX_RERANK_CANDIDATES-1));assert_eq!(hits[1]["sourceId"],"s0000");
 // Rerank disabled: cosine order, and the provider is not called.
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled"}}})).unwrap();
 let hits=invoke(&w,"memory_search",body.clone()).unwrap();assert_eq!((hits[0]["sourceId"].as_str(),hits[1]["sourceId"].as_str()),(Some("s0000"),Some("s0001")));assert_eq!(seen.lock().unwrap().len(),1);
 // Exactly the scan ceiling is searchable (foreign rows are not counted); one more row is an explicit error before any provider call.
 let c=database::db::get_connection().unwrap();insert(&c,owner,MAX_RERANK_CANDIDATES+20..MAX_CANDIDATES);drop(c);
 assert_eq!(invoke(&w,"memory_search",body.clone()).unwrap().as_array().unwrap().len(),2);
 let c=database::db::get_connection().unwrap();insert(&c,owner,MAX_CANDIDATES..MAX_CANDIDATES+1);drop(c);
 let err=invoke(&w,"memory_search",body.clone()).unwrap_err();assert!(err.to_string().contains("memory_search_candidate_limit"),"{err}");
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"zenmux_api","model":"qwen/qwen3-vl-rerank","baseUrl":format!("{}/api/v1",server.uri())}}})).unwrap();
 assert!(invoke(&w,"memory_search",body).unwrap_err().to_string().contains("memory_search_candidate_limit"));assert_eq!(seen.lock().unwrap().len(),1,"the cap fails before the rerank provider is contacted");
 });
}

#[test]
fn memory_index_status_registered_retry_exhaustion_is_visible_and_owner_scoped(){
 // Real production worker functions: a job that cannot be embedded is retried, then ends terminal with its reason visible through the registered status command.
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 agent_core::foundation::db_bridge::register(database::db::get_connection);
 tauri::async_runtime::block_on(async {
 use agent_core::core::session::journey_embedding as emb;
 let state=agent_core::state::AgentAppState::new();let (owner,other)=("status-owner","status-other");
 for o in [owner,other]{state.register_session(agent_core::state::AgentSession::new(o.into(),agent_core::definitions::sde_agent())).await;}
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![crate::memory_search_commands::memory_index_status,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 // A config that does not match the pinned space fails the job explicitly (no fallback provider), independent of any credential.
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1536}}})).unwrap();
 let config_state=state.clone();emb::register_pinned_embedding(emb::PinnedJourneyEmbedding{provider:std::sync::Arc::new(crate::journey_embedding_provider::test_provider(std::sync::Arc::new(move||config_state.integrations.snapshot().embedding))),model:crate::memory_search_commands::MODEL.into(),dimensions:1024,source:crate::memory_search_commands::SOURCE.into()}).unwrap();
 let c=database::db::get_connection().unwrap();emb::enqueue(&c,"fact:never-embeds",emb::JourneyEmbeddingKind::ConfirmedFact,owner,"f","r","content that cannot be embedded").unwrap();drop(c);
 let status=|o:&str|invoke(&w,"memory_index_status",json!({"ownerSessionId":o}));
 for attempt in 1..=emb::MAX_EMBEDDING_ATTEMPTS{
  if attempt>1{let c=database::db::get_connection().unwrap();assert_eq!(emb::requeue_retryable_failures(&c).unwrap(),1);}
  assert!(emb::run_pinned_embedding_job().await);
  let st=status(owner).unwrap();let last=attempt==emb::MAX_EMBEDDING_ATTEMPTS;
  assert_eq!((st["failed"].as_i64(),st["terminalFailed"].as_i64(),st["indexed"].as_i64(),st["maxAttempts"].as_i64()),(Some(1),Some(last as i64),Some(0),Some(emb::MAX_EMBEDDING_ATTEMPTS)),"attempt {attempt}");
  let f=&st["failures"][0];assert_eq!((f["sourceId"].as_str(),f["attempt"].as_i64(),f["terminal"].as_bool(),f["error"].as_str()),(Some("fact:never-embeds"),Some(attempt),Some(last),Some("memory_query_config_fingerprint_mismatch")));
 }
 // Exhausted: no further automatic retry, even once the configuration is corrected; the failure stays visible.
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1024}}})).unwrap();
 let c=database::db::get_connection().unwrap();assert_eq!(emb::requeue_retryable_failures(&c).unwrap(),0);drop(c);assert!(!emb::run_pinned_embedding_job().await);
 let st=status(owner).unwrap();assert_eq!((st["terminalFailed"].as_i64(),st["failures"].as_array().unwrap().len()),(Some(1),1));
 // Another registered owner sees none of it; unregistered and CLI owners are refused.
 let st=status(other).unwrap();assert_eq!((st["failed"].as_i64(),st["queued"].as_i64(),st["failures"].as_array().unwrap().len()),(Some(0),Some(0),0));
 assert!(status("not-registered").is_err());assert!(status("cli:old").is_err());
 });
}

#[test]
fn journey_owner_semantic_chain_producer_index_query_source_location(){
 // Real chain: journey_confirm (registered IPC) -> durable outbox -> pinned producer job -> index -> memory_search -> same-owner source anchor -> exact prefix preview.
 let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
 let _sandbox=test_helpers::test_env::sandbox();
 let _kv=key_vault_fixture();
 agent_core::foundation::db_bridge::register(database::db::get_connection);
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path};
 use agent_core::core::session::{journey_application_service as svc,journey_embedding as emb,journey_lifecycle as life};
 let server=MockServer::start().await;let mut vector=vec![0.0f32;1024];vector[0]=1.;
 Mock::given(method("POST")).and(path("/api/v1/embeddings")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":vector}],"model":"qwen/qwen3-vl-embedding","usage":{"prompt_tokens":30,"total_tokens":30,"cost":"0.000003","cost_details":{"prompt":"0.000003","completion":"0"}}}))).expect(7).mount(&server).await;
 let mut key=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::OpenaiApi);key.api_key=Some("synthetic-no-production-key".into());key.base_url=Some(format!("{}/api/v1",server.uri()));key.enabled_models=vec!["qwen/qwen3-vl-embedding".into()];key.health_status=key_vault::key_store::HealthStatus::Valid;key.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(key).unwrap();
 let state=agent_core::state::AgentAppState::new();let owner="semantic-chain-owner";
 state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())).await;
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-12T00:00:00Z','2026-10-12T00:00:00Z')",[owner]).unwrap();
 c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('sm-1',?1,'user','chain root',1,'2026-10-12T00:00:00Z')",[owner]).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![journey_snapshot,journey_boundary_preview,journey_task_start,journey_task_finish,journey_fork_start,journey_confirm,crate::memory_search_commands::memory_search,crate::memory_search_commands::memory_index_status,agent_core::definitions::commands::integrations_update_patch]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 invoke(&w,"integrations_update_patch",json!({"patch":{"embedding":{"provider":"embedding_api","model":"qwen/qwen3-vl-embedding","dimensions":1024},"rerank":{"provider":"disabled"}}})).unwrap();
 assert_eq!(invoke(&w,"journey_task_start",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":0,"taskId":"chain-root-task","name":"root","position":"最近用户消息"}})).unwrap()["revision"],1);
 let c=database::db::get_connection().unwrap();let tx=c.unchecked_transaction().unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"sm-1",1,"user").unwrap();
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('sm-2',?1,'user','chain endpoint',2,'2026-10-12T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"sm-2",2,"user").unwrap();tx.commit().unwrap();drop(c);
 let snapshot=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();let anchor=snapshot["nodes"].as_array().unwrap().iter().find(|n|n["anchor"]["durableMessageId"]=="sm-2").unwrap()["anchor"].clone();
 assert_eq!(invoke(&w,"journey_task_finish",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":1,"outcome":"completed","messageId":"sm-2"}})).unwrap()["revision"],2);
 assert_eq!(invoke(&w,"journey_fork_start",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":2,"forkId":"chain-fork","taskId":"chain-fork-task","taskName":"fork","anchorMessageId":"sm-2"},"anchor":anchor})).unwrap()["revision"],3);
 let c=database::db::get_connection().unwrap();let tx=c.unchecked_transaction().unwrap();
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('sm-child',?1,'assistant','chain child evidence',3,'2026-10-12T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"sm-child",3,"assistant").unwrap();tx.commit().unwrap();
 let mut c=c;
 svc::SessionJourneyApplicationService::request_fork_close(&mut c,svc::RequestForkCloseRequest{session_id:owner.into(),expected_revision:3,fork_id:"chain-fork".into(),review_id:"chain-review".into(),outcome:life::TaskOutcome::Completed,message_id:"sm-child".into()}).unwrap();
 svc::SessionJourneyApplicationService::mark_review_ready(&mut c,svc::MarkReviewReadyRequest{session_id:owner.into(),expected_revision:4,review_id:"chain-review".into(),provenance:life::RuntimeProvenance{model_id:"m".into(),account_id:"a".into(),protocol:"p".into()},annotation:"chain review annotation".into()}).unwrap();
 drop(c);
 assert_eq!(invoke(&w,"journey_confirm",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":5,"reviewId":"chain-review","factId":"chain-fact","text":"chain confirmed fact","evidenceStartMessageId":"sm-child","evidenceEndMessageId":"sm-child"}})).unwrap()["revision"],6);
 // The review-annotation producer path is the review queue's own enqueue; same outbox, same pinned worker.
 let c=database::db::get_connection().unwrap();emb::enqueue(&c,"review:chain-review",emb::JourneyEmbeddingKind::ReviewAnnotation,owner,"chain-fork","chain-review","chain review annotation").unwrap();
 let queued=|c:&rusqlite::Connection,st:&str|c.query_row("SELECT COUNT(*) FROM session_journey_embedding_outbox WHERE session_id=?1 AND state=?2",rusqlite::params![owner,st],|r|r.get::<_,i64>(0)).unwrap();
 assert_eq!(queued(&c,"queued"),2);drop(c);
 // Without a pinned provider the worker claims nothing and rows remain queued (no silent fallback).
 let config_state=state.clone();emb::register_pinned_embedding(emb::PinnedJourneyEmbedding{provider:std::sync::Arc::new(crate::journey_embedding_provider::test_provider(std::sync::Arc::new(move||config_state.integrations.snapshot().embedding))),model:crate::memory_search_commands::MODEL.into(),dimensions:1024,source:crate::memory_search_commands::SOURCE.into()}).unwrap();
 let mut ran=0;while emb::run_pinned_embedding_job().await{ran+=1;assert!(ran<5);}assert_eq!(ran,2);
 let c=database::db::get_connection().unwrap();assert_eq!(queued(&c,"completed"),2);
 assert_eq!(c.query_row("SELECT COUNT(*) FROM session_journey_embedding_index WHERE session_id=?1 AND embedding_source=?2 AND embedding_dimensions=1024",rusqlite::params![owner,crate::memory_search_commands::SOURCE],|r|r.get::<_,i64>(0)).unwrap(),2);
 // A foreign owner's identical vector must never surface for this owner.
 let blob:Vec<u8>=vector.iter().flat_map(|v|v.to_le_bytes()).collect();
 c.execute("INSERT INTO session_journey_embedding_index(source_id,source_kind,session_id,fork_id,review_id,content,embedding,embedding_model,embedding_source,embedding_dimensions) VALUES('fact:foreign','confirmed_fact','foreign-owner','f','r','foreign secret',?1,'qwen/qwen3-vl-embedding',?2,1024)",rusqlite::params![blob,crate::memory_search_commands::SOURCE]).unwrap();drop(c);
 let hits=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"semantic chain query","topN":10})).unwrap();let hits=hits.as_array().unwrap();
 assert_eq!(hits.len(),2);assert!(hits.iter().all(|h|h["sourceId"]!="fact:foreign"));
 for h in hits{assert_eq!(h["sourceAnchor"]["durableMessageId"],"sm-child");assert!(h["sourceUnavailable"].is_null());assert_eq!(h["sourceAnchor"]["ownerSessionId"],owner);}
 let fact=hits.iter().find(|h|h["sourceId"]=="fact:chain-fact").unwrap();assert_eq!(fact["sourceKind"],"confirmed_fact");assert_eq!(fact["content"],"chain confirmed fact");
 assert!(hits.iter().any(|h|h["sourceId"]=="review:chain-review"&&h["sourceKind"]=="review_annotation"));
 let preview=invoke(&w,"journey_boundary_preview",json!({"ownerSessionId":owner,"anchor":fact["sourceAnchor"]})).unwrap();let ids:Vec<_>=preview["records"].as_array().unwrap().iter().map(|r|r["durableMessageId"].as_str().unwrap().to_string()).collect();assert_eq!(ids.last().map(String::as_str),Some("sm-child"));
 // Pipeline state is visible to the owner: both jobs completed and indexed, nothing failed.
 let st=invoke(&w,"memory_index_status",json!({"ownerSessionId":owner})).unwrap();assert_eq!((st["completed"].as_i64(),st["indexed"].as_i64(),st["failed"].as_i64(),st["terminalFailed"].as_i64(),st["queued"].as_i64()),(Some(2),Some(2),Some(0),Some(0),Some(0)));assert!(st["failures"].as_array().unwrap().is_empty());
 assert!(invoke(&w,"memory_index_status",json!({"ownerSessionId":"foreign-owner"})).is_err(),"unregistered owner has no status");
 // Owner-internal rerank over the real index. The provider receives exactly this owner's candidates (the foreign row never leaves the owner), order and score come from the provider,
 // and every reranked hit still carries its own re-verified source anchor and kind.
 let mut rk=key_vault::key_store::ModelKey::new(key_vault::key_store::ModelType::ZenmuxApi);rk.api_key=Some("synthetic-zenmux-no-production-key".into());rk.base_url=Some(format!("{}/api/v1",server.uri()));rk.enabled=true;rk.enabled_models=vec!["qwen/qwen3-vl-rerank".into()];rk.health_status=key_vault::key_store::HealthStatus::Valid;rk.last_validated_at=Some(chrono::Utc::now());key_vault::key_store::KEY_SERVICE.save_key(rk).unwrap();
 Mock::given(method("POST")).and(path("/api/v1/rerank")).and(wiremock::matchers::body_json(json!({"model":"qwen/qwen3-vl-rerank","input":{"query":"semantic chain query","documents":["chain confirmed fact","chain review annotation"]},"parameters":{"top_n":2,"return_documents":true}}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[{"index":1,"relevance_score":0.9},{"index":0,"relevance_score":0.1}]}))).expect(1).mount(&server).await;
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"zenmux_api","model":"qwen/qwen3-vl-rerank","baseUrl":format!("{}/api/v1",server.uri())}}})).unwrap();
 let hits=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"semantic chain query","topN":10})).unwrap();let hits=hits.as_array().unwrap();assert_eq!(hits.len(),2);
 assert_eq!((hits[0]["sourceId"].as_str(),hits[0]["sourceKind"].as_str()),(Some("review:chain-review"),Some("review_annotation")));assert!((hits[0]["score"].as_f64().unwrap()-0.9).abs()<1e-6);
 assert_eq!((hits[1]["sourceId"].as_str(),hits[1]["sourceKind"].as_str()),(Some("fact:chain-fact"),Some("confirmed_fact")));assert!((hits[1]["score"].as_f64().unwrap()-0.1).abs()<1e-6);
 for h in hits{assert_eq!(h["sourceAnchor"]["durableMessageId"],"sm-child");assert_eq!(h["sourceAnchor"]["ownerSessionId"],owner);assert!(h["sourceUnavailable"].is_null());}
 // A provider failure is an explicit error, never cosine-ordered hits and never another provider.
 Mock::given(method("POST")).and(path("/api/v1/rerank")).respond_with(ResponseTemplate::new(503)).with_priority(1).expect(1).mount(&server).await;
 assert!(invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"semantic chain query","topN":10})).is_err());
 invoke(&w,"integrations_update_patch",json!({"patch":{"rerank":{"provider":"disabled"}}})).unwrap();
 // Index content that disagrees with the owner's Journey aggregate yields an explicit unavailable reason, never a guessed source.
 let c=database::db::get_connection().unwrap();c.execute("UPDATE session_journey_embedding_index SET content='tampered' WHERE source_id='fact:chain-fact'",[]).unwrap();c.execute("DELETE FROM session_journey_embedding_index WHERE source_id='review:chain-review'",[]).unwrap();drop(c);
 let hits=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"semantic chain query","topN":10})).unwrap();assert_eq!(hits.as_array().unwrap().len(),1);
 assert!(hits[0]["sourceAnchor"].is_null());assert_eq!(hits[0]["sourceUnavailable"],"source_fact_mismatch");
 // Deleting the evidence message makes the source unresolvable, again explicitly.
 let c=database::db::get_connection().unwrap();c.execute("UPDATE session_journey_embedding_index SET content='chain confirmed fact' WHERE source_id='fact:chain-fact'",[]).unwrap();c.execute("DELETE FROM agent_messages WHERE id='sm-child'",[]).unwrap();drop(c);
 let hits=invoke(&w,"memory_search",json!({"ownerSessionId":owner,"query":"semantic chain query","topN":10})).unwrap();assert_eq!(hits[0]["sourceUnavailable"],"source_anchor_unresolvable");
 server.verify().await;
 });
}

#[test]
fn historical_runtime_registered_exact_prefix_intent_replay(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="osagent-history-source";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-10T00:00:00Z','2026-10-10T00:00:00Z')",[owner]).unwrap();
 for (id,seq,role) in [("h-user",1,"user"),("h-answer",2,"assistant"),("h-suffix",3,"user")]{c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?1,?4,'2026-10-10T00:00:00Z')",rusqlite::params![id,owner,role,seq]).unwrap();}
 let anchor=agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"h-answer",3).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![historical_runtime_fork]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let request=json!({"ownerSessionId":owner,"request":{"anchor":anchor,"intentId":"intent-one"}});
 let result=invoke(&w,"historical_runtime_fork",request.clone()).unwrap();let child=result["childSessionId"].as_str().unwrap();assert_ne!(child,owner);
 assert!(tauri::async_runtime::block_on(state.get_session(child)).is_some());
 assert_eq!(invoke(&w,"historical_runtime_fork",request.clone()).unwrap(),result);
 let c=database::db::get_connection().unwrap();let rows=agent_core::session::persistence::load_messages(child).unwrap();assert_eq!(rows.iter().map(|r|r.content.as_str()).collect::<Vec<_>>(),vec!["h-user","h-answer"]);
 assert_eq!(c.query_row("SELECT parent_session_id FROM agent_sessions WHERE session_id=?1",[child],|r|r.get::<_,String>(0)).unwrap(),owner);drop(c);
 tauri::async_runtime::block_on(state.remove_session(child));assert_eq!(invoke(&w,"historical_runtime_fork",request.clone()).unwrap(),result);
 let mut independent=request.clone();independent["request"]["intentId"]=json!("intent-two");assert_ne!(invoke(&w,"historical_runtime_fork",independent).unwrap()["childSessionId"],result["childSessionId"]);
 let mut mismatch=request.clone();mismatch["request"]["anchor"]["boundaryHash"]=json!("changed");assert!(invoke(&w,"historical_runtime_fork",mismatch).is_err());
 let mut foreign=request;foreign["request"]["anchor"]["sourceSessionId"]=json!("foreign");foreign["request"]["intentId"]=json!("foreign-intent");assert!(invoke(&w,"historical_runtime_fork",foreign).is_err());
 assert_eq!(agent_core::session::persistence::load_messages(owner).unwrap().len(),3);
}

#[test]
fn historical_runtime_revocation_at_writer_boundary(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="osagent-history-revoke";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-10','2026-10-10')",[owner]).unwrap();
 c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('revoke-root',?1,'user','root',1,'2026-10-10T00:00:00Z')",[owner]).unwrap();
 let anchor=agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"revoke-root",1).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![historical_runtime_fork]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let (entered_tx,entered_rx)=std::sync::mpsc::channel();
 let (resume_tx,resume_rx)=tokio::sync::oneshot::channel();
 *HISTORICAL_FORK_TEST_GATE.lock().unwrap()=Some((entered_tx,resume_rx));
 let worker=database::db::with_sessions_writer(||{
  let worker=std::thread::spawn(move||invoke(&w,"historical_runtime_fork",json!({"ownerSessionId":owner,"request":{"anchor":anchor,"intentId":"revoked"}})));
  entered_rx.recv().unwrap();
  tauri::async_runtime::block_on(state.remove_session(owner));
  resume_tx.send(()).unwrap();worker
 });
 assert!(worker.join().unwrap().is_err(),"revoked runtime must not commit a child");
 let c=database::db::get_connection().unwrap();let children:i64=c.query_row("SELECT COUNT(*) FROM agent_sessions WHERE parent_session_id=?1",[owner],|r|r.get(0)).unwrap();assert_eq!(children,0);
}


// Dedicated historical runtime fixtures: not Journey/compaction fork coverage.
#[test]
fn historical_runtime_concurrent_transaction_reopen_usage_isolation(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="osagent-history-transaction";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'fixture','fixture')",[owner]).unwrap();
 for (id,seq,role) in [("tx-user",1,"user"),("tx-answer",2,"assistant"),("tx-late",3,"user")]{c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?1,?4,'fixture')",rusqlite::params![id,owner,role,seq]).unwrap();}
 let anchor=agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"tx-answer",3).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![historical_runtime_fork]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let request=json!({"ownerSessionId":owner,"request":{"anchor":anchor,"intentId":"concurrent"}});
 // Registry and processing try-lock busy are retryable contracts, not failures.
 let retry=|window:&tauri::WebviewWindow<MockRuntime>,body:Value| {for _ in 0..500 {match invoke(window,"historical_runtime_fork",body.clone()){Ok(v)=>return v,Err(e)=>{let e=e.to_string();assert!(e.contains("journey_session_busy")||e.contains("journey_registry_busy"),"{e}");std::thread::sleep(std::time::Duration::from_millis(2));}}}panic!("historical fork busy did not settle")};
 let barrier=std::sync::Arc::new(std::sync::Barrier::new(4));
 let workers=(0..4).map(|_|{let w=w.clone();let r=request.clone();let b=barrier.clone();std::thread::spawn(move||{b.wait();retry(&w,r)})}).collect::<Vec<_>>();
 let results=workers.into_iter().map(|w|w.join().unwrap()).collect::<Vec<_>>();for r in &results{assert_eq!(r,&results[0]);}let child=results[0]["childSessionId"].as_str().unwrap().to_string();
 let independent=(0..2).map(|i|{let w=w.clone();let mut r=request.clone();r["request"]["intentId"]=json!(format!("independent-{i}"));std::thread::spawn(move||retry(&w,r))}).collect::<Vec<_>>().into_iter().map(|t|t.join().unwrap()).collect::<Vec<_>>();assert_ne!(independent[0]["childSessionId"],independent[1]["childSessionId"]);assert_ne!(independent[0]["childSessionId"],json!(child));
 // Abort each durable write phase independently; SQLite must leave no half-child.
 for (phase,table) in [("seed","agent_sessions"),("messages","agent_messages"),("intent","historical_runtime_fork_intents")]{
  let c=database::db::get_connection().unwrap();let before:i64=c.query_row("SELECT COUNT(*) FROM agent_sessions",[],|r|r.get(0)).unwrap();
  c.execute_batch(&format!("CREATE TRIGGER historical_fail BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'historical injected {phase}'); END;")).unwrap();drop(c);
  let mut r=request.clone();r["request"]["intentId"]=json!(format!("rollback-{phase}"));let error=invoke(&w,"historical_runtime_fork",r.clone()).unwrap_err().to_string();assert!(error.contains("historical injected"),"{error}");
  let c=database::db::get_connection().unwrap();assert_eq!(c.query_row("SELECT COUNT(*) FROM agent_sessions",[],|r|r.get::<_,i64>(0)).unwrap(),before);assert_eq!(c.query_row("SELECT COUNT(*) FROM historical_runtime_fork_intents WHERE intent_id=?1",[format!("rollback-{phase}")],|r|r.get::<_,i64>(0)).unwrap(),0);assert_eq!(c.query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id NOT IN (SELECT session_id FROM agent_sessions)",[],|r|r.get::<_,i64>(0)).unwrap(),0);c.execute_batch("DROP TRIGGER historical_fail").unwrap();drop(c);retry(&w,r);
 }
 // Open a fresh physical SQLite handle (not the pool), then rehydrate/replay.
 tauri::async_runtime::block_on(state.remove_session(&child));
 let c=rusqlite::Connection::open(database::db::get_db_path()).unwrap();
 assert_eq!(c.query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id=?1",[&child],|r|r.get::<_,i64>(0)).unwrap(),2);
 c.execute("INSERT INTO session_token_usage(session_id,session_type,total_tokens,created_at) VALUES(?1,'sde',101,'fixture')",[owner]).unwrap();c.execute("INSERT INTO session_token_usage(session_id,session_type,total_tokens,created_at) VALUES(?1,'sde',7,'fixture')",[&child]).unwrap();
 assert_eq!(c.query_row("SELECT SUM(total_tokens) FROM session_token_usage WHERE session_id=?1",[owner],|r|r.get::<_,i64>(0)).unwrap(),101);assert_eq!(c.query_row("SELECT SUM(total_tokens) FROM session_token_usage WHERE session_id=?1",[&child],|r|r.get::<_,i64>(0)).unwrap(),7);drop(c);
 assert_eq!(retry(&w,request),results[0]);assert!(tauri::async_runtime::block_on(state.get_session(&child)).is_some());
 assert_eq!(agent_core::session::persistence::load_messages(&child).unwrap().iter().map(|m|m.content.as_str()).collect::<Vec<_>>(),vec!["tx-user","tx-answer"]);
}

#[test]
fn historical_runtime_registered_rejects_unpaired_inclusive_endpoint(){
 let _sandbox=test_helpers::test_env::sandbox();let owner="osagent-history-pair";let state=agent_core::state::AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'fixture','fixture')",[owner]).unwrap();
 for(id,seq,role)in[("u",1,"user"),("call",2,"tool_call"),("result",3,"tool_result")]{c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at,tool_call_id) VALUES(?1,?2,?3,?1,?4,'fixture','tool-one')",rusqlite::params![id,owner,role,seq]).unwrap();}
 let anchor=agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"call",3).unwrap();
 let legal=agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"result",3).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![historical_runtime_fork]).build(mock_context(noop_assets())).unwrap();let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 assert!(invoke(&w,"historical_runtime_fork",json!({"ownerSessionId":owner,"request":{"anchor":anchor,"intentId":"incomplete"}})).unwrap_err().to_string().contains("historical_fork_unpaired_tool_prefix"));
 let result=invoke(&w,"historical_runtime_fork",json!({"ownerSessionId":owner,"request":{"anchor":legal,"intentId":"complete"}})).unwrap();assert!(tauri::async_runtime::block_on(state.get_session(result["childSessionId"].as_str().unwrap())).is_some());
}

// R3 small matrix: two registered owners share ordinal coordinates and intent ids.
fn seed_history_owner(state:&agent_core::state::AgentAppState,owner:&str){
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'fixture','fixture')",[owner]).unwrap();
 for(suffix,seq,role)in[("user",1,"user"),("answer",2,"assistant"),("suffix",3,"user")]{c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?1,?4,'fixture')",rusqlite::params![format!("{owner}-{suffix}"),owner,role,seq]).unwrap();}
}
fn history_anchor(owner:&str,suffix:&str)->Value{
 let c=database::db::get_connection().unwrap();
 serde_json::to_value(agent_core::core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),&format!("{owner}-{suffix}"),3).unwrap()).unwrap()
}
#[test]
fn historical_runtime_two_owners_same_ordinal_interleaved_a_b_a(){
 let _sandbox=test_helpers::test_env::sandbox();let (a,b)=("osagent-history-ord-a","osagent-history-ord-b");let state=agent_core::state::AgentAppState::new();
 seed_history_owner(&state,a);seed_history_owner(&state,b);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![historical_runtime_fork]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let fork=|owner:&str,suffix:&str,intent:&str|invoke(&w,"historical_runtime_fork",json!({"ownerSessionId":owner,"request":{"anchor":history_anchor(owner,suffix),"intentId":intent}}));
 // A: first fork. Then A's turn is busy while B forks the same ordinal + intent.
 let a1=fork(a,"answer","same-intent").unwrap();
 let a_session=tauri::async_runtime::block_on(state.get_session(a)).unwrap();let a_turn=a_session.processing_lock.clone().try_lock_owned().unwrap();
 let b1=fork(b,"answer","same-intent").unwrap();
 assert!(fork(a,"answer","second-intent").unwrap_err().to_string().contains("journey_session_busy"),"A busy must not block or be served by B");
 drop(a_turn);
 // A again, after B committed: parent identity follows the request owner only.
 let a2=fork(a,"answer","second-intent").unwrap();
 let children=[&a1,&b1,&a2].map(|r|r["childSessionId"].as_str().unwrap().to_string());
 assert_eq!(std::collections::HashSet::<&String>::from_iter(&children).len(),3,"same ordinal/intent must not collide across owners");
 assert!(children[0].starts_with(&format!("{a}-history-")));assert!(children[1].starts_with(&format!("{b}-history-")));assert!(children[2].starts_with(&format!("{a}-history-")));
 let c=database::db::get_connection().unwrap();
 for(child,owner)in[(&children[0],a),(&children[1],b),(&children[2],a)]{
  assert_eq!(c.query_row("SELECT parent_session_id FROM agent_sessions WHERE session_id=?1",[child],|r|r.get::<_,String>(0)).unwrap(),owner);
  let rows=agent_core::session::persistence::load_messages(child).unwrap();
  assert_eq!(rows.iter().map(|r|r.content.clone()).collect::<Vec<_>>(),vec![format!("{owner}-user"),format!("{owner}-answer")],"exclusive end + no cross-owner rows");
 }
 let intents=|owner:&str|c.query_row("SELECT COUNT(*) FROM historical_runtime_fork_intents WHERE owner_session_id=?1",[owner],|r|r.get::<_,i64>(0)).unwrap();
 assert_eq!((intents(a),intents(b)),(2,1));drop(c);
 // Same owner + intent with a different endpoint is an exact conflict, not a new child.
 assert_eq!(fork(a,"user","same-intent").unwrap_err(),json!("historical_fork_intent_conflict"));
 assert_eq!(fork(b,"user","same-intent").unwrap_err(),json!("historical_fork_intent_conflict"));
 // Owner B cannot replay or fork through A's anchor, even with A's intent id.
 assert!(invoke(&w,"historical_runtime_fork",json!({"ownerSessionId":b,"request":{"anchor":history_anchor(a,"answer"),"intentId":"same-intent"}})).is_err());
 // Replay after both runtimes are removed and re-registered stays owner-scoped.
 for child in &children{tauri::async_runtime::block_on(state.remove_session(child));}
 assert_eq!(fork(a,"answer","same-intent").unwrap(),a1);assert_eq!(fork(b,"answer","same-intent").unwrap(),b1);assert_eq!(fork(a,"answer","second-intent").unwrap(),a2);
 assert_eq!(agent_core::session::persistence::load_messages(a).unwrap().len(),3);assert_eq!(agent_core::session::persistence::load_messages(b).unwrap().len(),3);
}

#[path="journey_historical_fork_ownership_tests.rs"]
mod historical_fork_ownership_tests;
