
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
