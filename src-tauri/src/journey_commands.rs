
//! Native canonical Journey snapshot read adapter. No Node scan or JSONL ordinal identity.
use serde::Serialize;
use agent_core::core::session::journey_canonical::DurableAnchor;
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct CanonicalNode{pub id:String,pub title:String,pub timestamp:i64,pub lane_id:String,pub kind:&'static str,pub anchor:DurableAnchor}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Capabilities{pub read:bool,pub fork:bool,pub import:bool,pub mutate:bool}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct JourneySnapshot{pub schema_version:u32,pub owner_session_id:String,pub revision:u64,pub nodes:Vec<CanonicalNode>,pub edges:Vec<serde_json::Value>,pub capabilities:Capabilities}
#[tauri::command]
pub async fn journey_snapshot(owner_session_id:String,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<JourneySnapshot,String>{
 // Canonical registered AgentSession identity is required. A selected timeline
 // label or imported provider path is not authority to initialize an owner.
 if owner_session_id.is_empty()||owner_session_id.starts_with("cli:"){return Err("journey_source_not_canonical".into());}
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_not_registered".into());}
 read_canonical_snapshot(&owner_session_id)
}
/// Explicit selected-session open, unlike read-only render. Never creates a durable owner.
#[tauri::command]
pub async fn journey_owner_open(owner_session_id:String,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<JourneySnapshot,String>{
 if owner_session_id.is_empty()||owner_session_id.starts_with("cli:"){return Err("journey_source_not_canonical".into());}
 {
 let conn=database::db::get_connection().map_err(|e|e.to_string())?;
 let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE session_id=?1)",[&owner_session_id],|row|row.get(0)).map_err(|e|e.to_string())?;
 if !exists{return Err("journey_owner_not_durable".into());}
 }
 if state.get_session(&owner_session_id).await.is_none(){agent_core::init::register_session_with_rehydrate(&state,&owner_session_id).await?;}
 read_canonical_snapshot(&owner_session_id)
}
fn read_canonical_snapshot(owner:&str)->Result<JourneySnapshot,String>{
 let conn=database::db::get_connection().map_err(|e|e.to_string())?;
 let rows=agent_core::session::persistence::load_messages(owner).map_err(|e|e.to_string())?;
 if rows.len()>10000{return Err("journey_snapshot_limit".into());}
 // Display revision is derived from durable message identity, not UI arrays.
 let revision=rows.iter().map(|r|r.sequence.max(0) as u64).max().unwrap_or(0);
 let branch=format!("session:{owner}");let mut nodes=Vec::new();
 for row in rows.into_iter().filter(|r|r.role=="user"||r.compact_from_sequence.is_some()){
  let timestamp=chrono::DateTime::parse_from_rfc3339(&row.created_at).map_err(|_|"journey_invalid_durable_time")?.timestamp_millis();
  let anchor=agent_core::core::session::journey_canonical::resolve_with_connection(&conn,owner,owner,&branch,&row.id,revision)?;
  let title=if row.compact_from_sequence.is_some(){"Context compacted".into()}else{row.content.chars().take(160).collect()};
  nodes.push(CanonicalNode{id:format!("{owner}:{}:{}",branch,row.id),title,timestamp,lane_id:branch.clone(),kind:if row.compact_from_sequence.is_some(){"verify"}else{"question"},anchor});
 }
 // Mutation permissions are not inferred from a read or a UI selection.
 Ok(JourneySnapshot{schema_version:1,owner_session_id:owner.into(),revision,nodes,edges:vec![],capabilities:Capabilities{read:true,fork:false,import:false,mutate:false}})
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn canonical_snapshot_wire_identity_and_no_mutation_claim(){let s=JourneySnapshot{schema_version:1,owner_session_id:"A".into(),revision:3,nodes:vec![],edges:vec![],capabilities:Capabilities{read:true,fork:false,import:false,mutate:false}};let v=serde_json::to_value(s).unwrap();assert_eq!(v["ownerSessionId"],"A");assert_eq!(v["schemaVersion"],1);assert_eq!(v["capabilities"]["fork"],false);assert_eq!(v["capabilities"]["mutate"],false);}
}

#[derive(Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct BoundaryPreview{pub owner_session_id:String,pub anchor:DurableAnchor,pub prefix_hash:String,pub records:Vec<PreviewRecord>}
#[derive(Debug,Serialize)]#[serde(rename_all="camelCase")]
pub struct PreviewRecord{pub durable_message_id:String,pub sequence:i64,pub role:String,pub content:String}
#[tauri::command]
pub async fn journey_boundary_preview(owner_session_id:String,anchor:DurableAnchor,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<BoundaryPreview,String>{
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_not_registered".into());}
 if anchor.owner_session_id!=owner_session_id||anchor.source_session_id!=owner_session_id{return Err("journey_source_unauthorized".into());}
 let conn=database::db::get_connection().map_err(|e|e.to_string())?;
 let result=read_boundary_preview(&conn,&owner_session_id,&anchor)?;
 // Native same-owner read authority is rechecked before publishing; no mutation inferred.
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}Ok(result)
}
fn read_boundary_preview(conn:&rusqlite::Connection,owner:&str,anchor:&DurableAnchor)->Result<BoundaryPreview,String>{
 if anchor.source_session_id!=owner{return Err("journey_source_unauthorized".into());}
 let tx=conn.unchecked_transaction().map_err(|e|e.to_string())?;
 let current_revision:i64=tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM agent_messages WHERE session_id=?1",[owner],|r|r.get(0)).map_err(|e|e.to_string())?;if current_revision<0||current_revision as u64!=anchor.boundary_version{return Err("journey_revision_changed".into());}
 agent_core::core::session::journey_canonical::validate_commit_with_connection(&tx,owner,anchor,anchor.boundary_version,true)?;
 let end:i64=tx.query_row("SELECT sequence FROM agent_messages WHERE session_id=?1 AND id=?2",rusqlite::params![owner,anchor.durable_message_id],|r|r.get(0)).map_err(|e|e.to_string())?;
 let records={let mut stmt=tx.prepare("SELECT id,sequence,role,content FROM agent_messages WHERE session_id=?1 AND sequence<=?2 ORDER BY sequence,id LIMIT 10001").map_err(|e|e.to_string())?;
 let rows=stmt.query_map(rusqlite::params![owner,end],|r|Ok(PreviewRecord{durable_message_id:r.get(0)?,sequence:r.get(1)?,role:r.get(2)?,content:r.get(3)?})).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;rows};
 if records.len()>10000||records.iter().map(|r|r.content.len()).sum::<usize>()>2*1024*1024{return Err("journey_preview_limit".into());}
 if records.last().map(|r|r.durable_message_id.as_str())!=Some(anchor.durable_message_id.as_str()){return Err("journey_ambiguous_durable_boundary".into());}
 let wire=serde_json::to_vec(&records).map_err(|e|e.to_string())?;use sha2::Digest;let prefix_hash=format!("{:x}",sha2::Sha256::digest(&wire));tx.commit().map_err(|e|e.to_string())?;
 Ok(BoundaryPreview{owner_session_id:owner.into(),anchor:anchor.clone(),prefix_hash,records})
}
#[cfg(test)]mod boundary_tests{use super::*;
 #[test]fn earlier_explicit_boundary_preview_hash_is_exact_and_fails_closed_after_edit_delete_wrong_owner(){
 let c=rusqlite::Connection::open_in_memory().unwrap();c.execute_batch("CREATE TABLE agent_messages(id TEXT,session_id TEXT,sequence INTEGER,created_at TEXT,role TEXT,content TEXT,compact_from_sequence INTEGER,PRIMARY KEY(session_id,id));INSERT INTO agent_messages VALUES('earlier','A',1,'fixture','user','original',NULL),('later','A',2,'fixture','user','later',NULL),('earlier','B',1,'fixture','user','unrelated',NULL);").unwrap();
 let a=agent_core::core::session::journey_canonical::resolve_with_connection(&c,"A","A","session:A","earlier",2).unwrap();let p=read_boundary_preview(&c,"A",&a).unwrap();assert_eq!(p.records.len(),1);assert_eq!(p.records[0].content,"original");assert!(!p.prefix_hash.is_empty());assert!(read_boundary_preview(&c,"B",&a).is_err());c.execute("UPDATE agent_messages SET content='changed' WHERE session_id='A' AND id='earlier'",[]).unwrap();assert!(read_boundary_preview(&c,"A",&a).is_err());c.execute("DELETE FROM agent_messages WHERE session_id='A' AND id='earlier'",[]).unwrap();assert!(read_boundary_preview(&c,"A",&a).is_err());
 }
}

#[tauri::command]
pub async fn journey_domain_snapshot(owner_session_id:String,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<agent_core::core::session::journey_application_service::JourneySnapshotResponse,String>{if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_not_registered".into());}let conn=database::db::get_connection().map_err(|e|e.to_string())?;agent_core::core::session::journey_application_service::SessionJourneyApplicationService::snapshot(&conn,&owner_session_id).map_err(|e|e.to_string())}

// Full historical lifecycle command batch. Actual canonical runtime identity and processing lock
// are checked before mutation. Provider execution is never initiated by these commands.
use agent_core::core::session::journey_application_service as journey_service;
fn check_journey_owner(owner:&str,request_owner:&str)->Result<(),String>{if owner.is_empty()||owner.starts_with("cli:")||owner!=request_owner{return Err("journey_owner_mismatch".into());}Ok(())}
async fn journey_mutation_lease(owner:&str,state:&agent_core::state::AgentAppState)->Result<tokio::sync::OwnedMutexGuard<()>,String>{
 let session=state.get_session(owner).await.ok_or("journey_owner_not_registered")?;
 session.processing_lock.clone().try_lock_owned().map_err(|_|"journey_session_busy".into())
}

#[tauri::command]
pub async fn journey_task_start(owner_session_id:String,request:journey_service::CreateTaskRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::create_task(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_entry_activate(owner_session_id:String,request:journey_service::ActivateJourneyEntryRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::activate_entry(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_checkpoint(owner_session_id:String,request:journey_service::CreateCheckpointRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::create_checkpoint(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_task_finish(owner_session_id:String,request:journey_service::FinishTaskRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::finish_task(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_review_retry(owner_session_id:String,request:journey_service::RetryReviewRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::retry_review(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_confirm(owner_session_id:String,request:journey_service::PromoteFactRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::promote_confirmed_fact(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_discard(owner_session_id:String,request:journey_service::DiscardForkRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::DiscardForkResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::discard_fork(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_return_parent(owner_session_id:String,request:journey_service::ReturnToParentRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::ReturnToParentResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||{database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::return_to_parent(&mut c,request).map_err(|e|e.to_string())})}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn journey_fork_start(owner_session_id:String,request:journey_service::CreateForkRequest,anchor:DurableAnchor,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<journey_service::JourneyWriteResponse,String>{
 check_journey_owner(&owner_session_id,&request.session_id)?;if request.anchor_message_id.as_deref()!=Some(anchor.durable_message_id.as_str())||anchor.source_session_id!=owner_session_id{return Err("journey_explicit_anchor_required".into());}
 let _lease=journey_mutation_lease(&owner_session_id,&state).await?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_revoked".into());}
 tokio::task::spawn_blocking(move||database::db::with_sessions_writer(||{let mut c=database::db::get_connection().map_err(|e|e.to_string())?;
 // Holding actual same-session processing lease + shared writer serializes canonical hash check
 // and service CAS against normal user/assistant persistence. Explicit ID never latest fallback.
 let _:BoundaryPreview=read_boundary_preview(&c,&owner_session_id,&anchor)?;
 journey_service::SessionJourneyApplicationService::create_fork_exact(&mut c,request,&anchor).map_err(|e|e.to_string())})).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn journey_review_list(owner_session_id:String,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<Vec<agent_core::core::session::journey_lifecycle::ReviewItem>,String>{
 if state.get_session(&owner_session_id).await.is_none(){return Err("journey_owner_not_registered".into());}let c=database::db::get_connection().map_err(|e|e.to_string())?;journey_service::SessionJourneyApplicationService::review_queue(&c,&owner_session_id).map_err(|e|e.to_string())
}
#[cfg(test)]mod lifecycle_owner_tests{use super::*;#[test]fn all_lifecycle_commands_require_exact_owner_not_ui_title_or_imported_cli(){assert!(check_journey_owner("A","A").is_ok());assert!(check_journey_owner("A","B").is_err());assert!(check_journey_owner("cli:A","cli:A").is_err());assert!(check_journey_owner("","").is_err());}}

#[cfg(test)]
#[path="journey_registered_owner_ipc_tests.rs"]
mod registered_owner_ipc_tests;

/// Explicit physical historical-prefix fork; never a compaction or Journey branch.
#[derive(Debug,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct HistoricalRuntimeForkRequest{pub anchor:DurableAnchor,pub intent_id:String}
#[derive(Debug,serde::Serialize)]
#[serde(rename_all="camelCase")]
pub struct HistoricalRuntimeForkResponse{pub child_session_id:String,pub parent_session_id:String}
#[cfg(test)]
static HISTORICAL_FORK_TEST_GATE: std::sync::Mutex<Option<(std::sync::mpsc::Sender<()>,tokio::sync::oneshot::Receiver<()>)>> = std::sync::Mutex::new(None);
#[tauri::command]
pub async fn historical_runtime_fork(owner_session_id:String,request:HistoricalRuntimeForkRequest,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<HistoricalRuntimeForkResponse,String>{
 check_journey_owner(&owner_session_id,&request.anchor.owner_session_id)?;
 let admitted=state.get_session(&owner_session_id).await.ok_or("journey_owner_not_registered")?;
 let _lease=admitted.processing_lock.clone().try_lock_owned().map_err(|_|"journey_session_busy")?;
 if request.anchor.source_session_id!=owner_session_id{return Err("historical_fork_foreign_source_not_authorized".into());}
 if request.intent_id.trim().is_empty()||request.intent_id.len()>256{return Err("historical_fork_invalid_intent".into());}
 if agent_core::definitions::prefix_lookup::definition_for_session_id(&owner_session_id).is_none(){return Err("historical_fork_unsupported_runtime_identity".into());}
 #[cfg(test)]
 { let gate=HISTORICAL_FORK_TEST_GATE.lock().unwrap().take(); if let Some((entered,resume))=gate{entered.send(()).unwrap();resume.await.unwrap();} }
 let owner=owner_session_id.clone();
 let registry=state.sessions.clone();
 // Writer -> nonblocking registry admission: never wait on registry while owning writer.
 // Hold registry only over synchronous SQLite commit, never across await/rehydration.
 // Removal before this linearization rejects even intent replay; after commit the child
 // remains durable and replayable. Replacement runtimes cannot inherit old admission.
 let response=tokio::task::spawn_blocking(move||database::db::with_sessions_writer(||{
  let sessions=registry.try_lock().map_err(|_|"journey_registry_busy".to_string())?;
  if !sessions.get(&owner).is_some_and(|current|std::sync::Arc::ptr_eq(current,&admitted)){return Err("journey_owner_revoked".into());}
  historical_fork_commit(&owner,&request)
 })).await.map_err(|e|e.to_string())??;
 // Durable commit is the replay boundary. If rehydrate fails, retry the same intent;
 // never delete a committed child or redirect/archive its parent.
 if state.get_session(&response.child_session_id).await.is_none(){agent_core::init::register_session_with_rehydrate(&state,&response.child_session_id).await?;}
 Ok(response)
}
fn historical_fork_commit(owner:&str,request:&HistoricalRuntimeForkRequest)->Result<HistoricalRuntimeForkResponse,String>{
 use rusqlite::{params,OptionalExtension};
 let mut c=database::db::get_connection().map_err(|e|e.to_string())?;
 let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
 let result=(||->Result<HistoricalRuntimeForkResponse,String>{
 tx.execute_batch("CREATE TABLE IF NOT EXISTS historical_runtime_fork_intents(owner_session_id TEXT NOT NULL,intent_id TEXT NOT NULL,anchor_json TEXT NOT NULL,child_session_id TEXT NOT NULL UNIQUE,parent_session_id TEXT NOT NULL,PRIMARY KEY(owner_session_id,intent_id));").map_err(|e|e.to_string())?;
 let anchor_json=serde_json::to_string(&request.anchor).map_err(|e|e.to_string())?;
 let prior=tx.query_row("SELECT anchor_json,child_session_id,parent_session_id FROM historical_runtime_fork_intents WHERE owner_session_id=?1 AND intent_id=?2",params![owner,request.intent_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional().map_err(|e|e.to_string())?;
 if let Some((stored,child,parent))=prior{if stored!=anchor_json{return Err("historical_fork_intent_conflict".into());}return Ok(HistoricalRuntimeForkResponse{child_session_id:child,parent_session_id:parent});}
 let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE session_id=?1)",[owner],|r|r.get(0)).map_err(|e|e.to_string())?;
 if !exists{return Err("historical_fork_owner_missing".into());}
 let revision:i64=tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM agent_messages WHERE session_id=?1",[owner],|r|r.get(0)).map_err(|e|e.to_string())?;
 agent_core::core::session::journey_canonical::validate_commit_with_connection(&tx,owner,&request.anchor,revision as u64,true)?;
 let endpoint:i64=tx.query_row("SELECT sequence FROM agent_messages WHERE session_id=?1 AND id=?2",params![owner,request.anchor.durable_message_id],|r|r.get(0)).map_err(|e|e.to_string())?;
 // Reject ambiguous physical coordinates rather than silently include same-sequence suffix.
 let unique:i64=tx.query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND sequence=?2",params![owner,endpoint],|r|r.get(0)).map_err(|e|e.to_string())?;if unique!=1{return Err("historical_fork_ambiguous_endpoint".into());}
 // Validate the loader-visible inclusive prefix, never round the endpoint to a turn.
 // Compaction summaries are user projections; only their visible tail needs pairing.
 let compact_floor:Option<i64>=tx.query_row("SELECT compact_from_sequence FROM agent_messages WHERE session_id=?1 AND sequence<=?2 AND compact_from_sequence IS NOT NULL ORDER BY sequence DESC LIMIT 1",params![owner,endpoint],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
 let mut pending=std::collections::HashSet::<String>::new();
 {let mut stmt=tx.prepare("SELECT role,tool_call_id FROM agent_messages WHERE session_id=?1 AND sequence<=?2 AND sequence>=?3 AND compact_from_sequence IS NULL ORDER BY sequence,id").map_err(|e|e.to_string())?;
 let rows=stmt.query_map(params![owner,endpoint,compact_floor.unwrap_or(i64::MIN)],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).map_err(|e|e.to_string())?;
 for row in rows{let(role,id)=row.map_err(|e|e.to_string())?;match role.as_str(){
 "tool_call"=>{let id=id.filter(|v|!v.is_empty()).ok_or("historical_fork_unpaired_tool_prefix")?;if !pending.insert(id){return Err("historical_fork_unpaired_tool_prefix".into());}},
 "tool_result"=>{if !id.as_ref().is_some_and(|v|pending.remove(v)){return Err("historical_fork_unpaired_tool_prefix".into());}},
 "user"|"assistant"|"system"=>{if !pending.is_empty(){return Err("historical_fork_unpaired_tool_prefix".into());}},_=>{}
 }} }
 if !pending.is_empty(){return Err("historical_fork_unpaired_tool_prefix".into());}
 let child=format!("{}-history-{}",owner,uuid::Uuid::new_v4());
 tx.execute("INSERT INTO agent_sessions(session_id,name,status,model,account_id,workspace_path,created_at,updated_at,parent_session_id,session_type) SELECT ?1,name,'idle',model,account_id,workspace_path,?3,?3,session_id,session_type FROM agent_sessions WHERE session_id=?2",params![child,owner,chrono::Utc::now().to_rfc3339()]).map_err(|e|e.to_string())?;
 // Copy every physical transcript column, including tool and compact metadata,
 // but mint child-local message identity. Usage/snapshots/gateway bindings stay parent-owned.
 let columns={let mut s=tx.prepare("PRAGMA table_info(agent_messages)").map_err(|e|e.to_string())?;let v=s.query_map([],|r|r.get::<_,String>(1)).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;v};
 let quoted=columns.iter().map(|n|format!("\"{}\"",n.replace('"',"\"\""))).collect::<Vec<_>>();
 let expressions=columns.iter().zip(&quoted).map(|(n,q)|match n.as_str(){"id"=>"?1 || ':' || id".to_string(),"session_id"=>"?1".to_string(),_=>q.clone()}).collect::<Vec<_>>();
 tx.execute(&format!("INSERT INTO agent_messages({}) SELECT {} FROM agent_messages WHERE session_id=?2 AND sequence<=?3 ORDER BY sequence,id",quoted.join(","),expressions.join(",")),params![child,owner,endpoint]).map_err(|e|e.to_string())?;
 tx.execute("INSERT INTO historical_runtime_fork_intents VALUES(?1,?2,?3,?4,?1)",params![owner,request.intent_id,anchor_json,child]).map_err(|e|e.to_string())?;
 Ok(HistoricalRuntimeForkResponse{child_session_id:child,parent_session_id:owner.into()})
 })()?;
 tx.commit().map_err(|e|e.to_string())?;Ok(result)
}
