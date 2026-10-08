//! Explicit user-selected native import IPC. Registered runtime ownership is required.
use agent_core::session::journey_canonical::DurableAnchor;
use agent_core::session::context_import::ContextSnapshot;
use agent_core::state::AgentAppState;
async fn registered_owner(state:&AgentAppState,owner:&str)->Result<(),String>{
 if owner.is_empty()||owner.starts_with("cli:"){return Err("context_import_invalid_owner".into());}
 if state.get_session(owner).await.is_none(){return Err("context_import_owner_not_registered".into());}Ok(())
}
#[tauri::command]
pub async fn context_import_selected(owner_session_id:String,anchor:DurableAnchor,token_budget:usize,pinned:bool,state:tauri::State<'_,AgentAppState>)->Result<ContextSnapshot,String>{
 registered_owner(&state,&owner_session_id).await?;
 // A user invoke is explicit admission, not model-provided permission. Source cannot switch owners.
 if anchor.owner_session_id!=owner_session_id||anchor.source_session_id!=owner_session_id{return Err("context_import_source_unauthorized".into());}
 let session=state.get_session(&owner_session_id).await.ok_or("context_import_owner_revoked")?;
 let _lease=session.processing_lock.clone().try_lock_owned().map_err(|_|"context_import_session_busy")?;
 let _writer=database::db::sessions_writer_guard();
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 agent_core::session::context_import::import_with_connection(&c,&owner_session_id,&anchor.source_session_id,&anchor,token_budget,None,pinned)
}
#[tauri::command]
pub async fn context_import_list(owner_session_id:String,state:tauri::State<'_,AgentAppState>)->Result<Vec<serde_json::Value>,String>{
 registered_owner(&state,&owner_session_id).await?;
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 agent_core::session::context_import::provider_sections_with_connection(&c,&owner_session_id).map_err(|e|e.to_string())
}
#[cfg(test)]
#[path="context_import_ipc_tests.rs"]
mod ipc_tests;
