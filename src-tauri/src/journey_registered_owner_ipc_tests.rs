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
 tx.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('journey-owner-selected-end',?1,'user','Exact end of active real task',2,'2026-10-08T00:00:00Z')",[owner]).unwrap();
 agent_core::session::journey_runtime_persistence::assign_inserted_message(&tx,owner,"journey-owner-selected-end",2,"user").unwrap();tx.commit().unwrap();drop(c);
 let snapshot=invoke(&w,"journey_snapshot",json!({"ownerSessionId":owner})).unwrap();let node=snapshot["nodes"].as_array().unwrap().iter().find(|n|n["anchor"]["durableMessageId"]=="journey-owner-selected-end").unwrap();let anchor=node["anchor"].clone();assert_eq!(anchor["boundaryVersion"],2);
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
 let activate=json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":4,"taskId":"root-task","branchId":"main"}});
 let result=invoke(&w,"journey_entry_activate",activate).unwrap();assert_eq!(result["revision"],5);
 let session=tauri::async_runtime::block_on(state.get_session(owner)).unwrap();let lease=session.processing_lock.clone().try_lock_owned().unwrap();
 assert!(invoke(&w,"journey_task_start",json!({"ownerSessionId":owner,"request":{"sessionId":owner,"expectedRevision":5,"taskId":"busy-task","name":"denied","position":"下一条用户消息"}})).is_err());drop(lease);
 tauri::async_runtime::block_on(state.remove_session(owner));assert!(invoke(&w,"journey_domain_snapshot",json!({"ownerSessionId":owner})).is_err());
}
