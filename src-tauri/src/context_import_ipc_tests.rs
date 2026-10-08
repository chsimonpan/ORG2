//! Real Tauri dispatcher and private SQLite; this is native IPC, not rendered GUI acceptance.
use super::*;
use serde_json::{Value,json};
use tauri::test::{mock_builder,mock_context,noop_assets,MockRuntime};
fn invoke(w:&tauri::WebviewWindow<MockRuntime>,cmd:&str,body:Value)->Result<Value,Value>{
 tauri::test::get_ipc_response(w,tauri::webview::InvokeRequest{cmd:cmd.into(),callback:tauri::ipc::CallbackFn(0),error:tauri::ipc::CallbackFn(1),url:w.url().unwrap(),body:tauri::ipc::InvokeBody::Json(body),headers:Default::default(),invoke_key:tauri::test::INVOKE_KEY.into()}).map(|b|b.deserialize::<Value>().unwrap())
}
#[test]
fn native_context_import_registered_owner_selected_anchor_ipc_durable_and_fail_closed(){
 let _sandbox=test_helpers::test_env::sandbox();
 let owner="registered-native-context-import";
 let state=AgentAppState::new();
 tauri::async_runtime::block_on(state.register_session(agent_core::state::AgentSession::new(owner.into(),agent_core::definitions::sde_agent())));
 let c=database::db::get_connection().unwrap();
 agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
 c.execute("INSERT OR REPLACE INTO agent_sessions(session_id,created_at,updated_at) VALUES(?1,'2026-10-08T00:00:00Z','2026-10-08T00:00:00Z')",[owner]).unwrap();
 c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES('ipc-import-end',?1,'user','Native GUI selection durable text',7,'2026-10-08T00:00:00Z')",[owner]).unwrap();
 let anchor=agent_core::session::journey_canonical::resolve_with_connection(&c,owner,owner,&format!("session:{owner}"),"ipc-import-end",7).unwrap();drop(c);
 let app=mock_builder().manage(state.clone()).invoke_handler(tauri::generate_handler![context_import_selected,context_import_list,context_import_snapshots]).build(mock_context(noop_assets())).unwrap();
 let w=tauri::WebviewWindowBuilder::new(&app,"main",Default::default()).build().unwrap();
 let p=json!({"ownerSessionId":owner,"anchor":anchor,"tokenBudget":4096,"pinned":true});
 let session=tauri::async_runtime::block_on(state.get_session(owner)).unwrap();
 let lease=session.processing_lock.clone().try_lock_owned().unwrap();
 assert!(invoke(&w,"context_import_selected",p.clone()).is_err(),"busy owner must deny user mutation");drop(lease);
 let result=invoke(&w,"context_import_selected",p.clone()).unwrap();
 assert_eq!(result["targetSessionId"],owner);assert_eq!(result["sourceId"],owner);assert_eq!(result["anchor"]["boundaryVersion"],7);assert_eq!(result["pinned"],true);
 let list=invoke(&w,"context_import_list",json!({"ownerSessionId":owner})).unwrap();assert_eq!(list.as_array().unwrap().len(),1);assert!(list[0]["content"].as_str().unwrap().contains("Native GUI selection durable text"));
 let snapshots=invoke(&w,"context_import_snapshots",json!({"ownerSessionId":owner})).unwrap();assert_eq!(snapshots.as_array().unwrap().len(),1);assert_eq!(snapshots[0],result);
 let c=rusqlite::Connection::open(database::db::get_db_path()).unwrap();assert_eq!(c.query_row("SELECT COUNT(*) FROM session_context_imports_v1 WHERE target_session_id=?1",[owner],|r|r.get::<_,i64>(0)).unwrap(),1);
 let mut foreign=p.clone();foreign["anchor"]["sourceSessionId"]=Value::String("foreign".into());assert!(invoke(&w,"context_import_selected",foreign).is_err());
 c.execute("UPDATE agent_messages SET content='stale selected anchor' WHERE id='ipc-import-end'",[]).unwrap();assert!(invoke(&w,"context_import_selected",p.clone()).is_err());drop(c);
 // Source edits must not rewrite the previously imported immutable card.
 let reload=invoke(&w,"context_import_snapshots",json!({"ownerSessionId":owner})).unwrap();assert_eq!(reload[0],result);
 tauri::async_runtime::block_on(state.remove_session(owner));assert!(invoke(&w,"context_import_selected",p).is_err());assert!(invoke(&w,"context_import_list",json!({"ownerSessionId":owner})).is_err());assert!(invoke(&w,"context_import_snapshots",json!({"ownerSessionId":owner})).is_err());
}
