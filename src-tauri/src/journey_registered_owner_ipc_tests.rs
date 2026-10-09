//! Real native dispatcher registered-session roundtrip. No fake HTML owner or generated transcript.
use super::*;
use serde_json::{Value,json};
use tauri::test::{mock_builder,mock_context,noop_assets,MockRuntime};
fn invoke(w:&tauri::WebviewWindow<MockRuntime>,cmd:&str,body:Value)->Result<Value,Value>{
 tauri::test::get_ipc_response(w,tauri::webview::InvokeRequest{cmd:cmd.into(),callback:tauri::ipc::CallbackFn(0),error:tauri::ipc::CallbackFn(1),url:w.url().unwrap(),body:tauri::ipc::InvokeBody::Json(body),headers:Default::default(),invoke_key:tauri::test::INVOKE_KEY.into()}).map(|b|b.deserialize::<Value>().unwrap())
}
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
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path,body_json};
 let server=MockServer::start().await;let mut vector=vec![0.0f32;1024];vector[0]=1.;
 Mock::given(method("POST")).and(path("/api/v1/embeddings")).and(body_json(json!({"model":"qwen/qwen3-vl-embedding","input":["transport query"],"dimensions":1024}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"embedding":vector}]}))).expect(1).mount(&server).await;
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
 tauri::async_runtime::block_on(async {
 use wiremock::{Mock,MockServer,ResponseTemplate};use wiremock::matchers::{method,path,body_json};
 let server=MockServer::start().await;let mut vector=vec![0.0f32;1024];vector[0]=1.;
 Mock::given(method("POST")).and(path("/api/v1/embeddings")).and(body_json(json!({"model":"qwen/qwen3-vl-embedding","input":["transport query"],"dimensions":1024}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"embedding":vector}]}))).expect(4).mount(&server).await;
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
