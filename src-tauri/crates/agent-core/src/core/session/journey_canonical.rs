
//! Canonical Journey boundary identity. No timeline ordinal/label/JSONL line authorization.
//! Callers supply explicit owner/source authority; native/Gateway adapters must recheck it.
use rusqlite::{Connection,OptionalExtension,params};
use serde::{Serialize,Deserialize};
use sha2::{Digest,Sha256};
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct DurableAnchor {
    pub owner_session_id:String,pub source_session_id:String,pub branch_id:String,
    pub durable_message_id:String,pub durable_turn_id:Option<String>,
    pub boundary_version:u64,pub boundary_hash:String,
}
fn digest(parts:&[&str])->String {
    let mut h=Sha256::new();for part in parts{h.update((part.len() as u64).to_le_bytes());h.update(part.as_bytes());}format!("{:x}",h.finalize())
}
/// A branch is a durable source namespace, never a semantic topic or UI label.
pub fn resolve_with_connection(conn:&Connection,owner:&str,source:&str,branch:&str,message:&str,revision:u64)->Result<DurableAnchor,String>{
    if owner.is_empty()||source.is_empty()||message.is_empty()||branch!=format!("session:{source}"){return Err("journey_invalid_anchor".into());}
    let row=conn.query_row("SELECT sequence,created_at,role,content,compact_from_sequence FROM agent_messages WHERE session_id=?1 AND id=?2",params![source,message],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?))).optional().map_err(|e|e.to_string())?.ok_or("journey_message_missing")?;
    if row.0 < 0 || row.1.is_empty() {return Err("journey_invalid_durable_coordinate".into());}
    let revision_s=revision.to_string();let sequence=row.0.to_string();let boundary=row.4.map(|n|n.to_string()).unwrap_or_default();
    Ok(DurableAnchor{owner_session_id:owner.into(),source_session_id:source.into(),branch_id:branch.into(),durable_message_id:message.into(),durable_turn_id:None,boundary_version:revision,boundary_hash:digest(&[owner,source,branch,message,&revision_s,&sequence,&row.1,&row.2,&row.3,&boundary])})
}
/// Invoke again inside the mutation transaction after source/target authority recheck.
pub fn validate_commit_with_connection(conn:&Connection,owner:&str,anchor:&DurableAnchor,expected_revision:u64,authorized:bool)->Result<(),String>{
    if !authorized { return Err("journey_source_unauthorized".into()); }
    if owner != anchor.owner_session_id { return Err("journey_owner_mismatch".into()); }
    let current = resolve_with_connection(conn,owner,&anchor.source_session_id,&anchor.branch_id,&anchor.durable_message_id,expected_revision)?;
    if current != *anchor { return Err("journey_anchor_changed".into()); }
    Ok(())
}
#[cfg(test)]
mod tests {
 use super::*;
 fn fixture()->Connection {let conn=Connection::open_in_memory().unwrap();conn.execute_batch("CREATE TABLE agent_messages(id TEXT,session_id TEXT,sequence INTEGER,created_at TEXT,role TEXT,content TEXT,compact_from_sequence INTEGER,PRIMARY KEY(session_id,id));INSERT INTO agent_messages VALUES('same-id','A',1,'fixture','user','original',NULL);INSERT INTO agent_messages VALUES('same-id','B',1,'fixture','user','other-session',NULL);").unwrap();conn}
 #[test]fn exact_owner_and_permission_are_commit_boundaries(){let c=fixture();let a=resolve_with_connection(&c,"A","A","session:A","same-id",3).unwrap();assert!(validate_commit_with_connection(&c,"B",&a,3,true).is_err());assert!(validate_commit_with_connection(&c,"A",&a,3,false).is_err());assert!(validate_commit_with_connection(&c,"A",&a,3,true).is_ok());}
 #[test]fn changed_content_sameordinal_or_deleted_explicit_anchor_never_falls_back(){let c=fixture();let a=resolve_with_connection(&c,"A","A","session:A","same-id",3).unwrap();c.execute("UPDATE agent_messages SET content='replacement' WHERE session_id='A'",[]).unwrap();assert!(validate_commit_with_connection(&c,"A",&a,3,true).is_err());c.execute("DELETE FROM agent_messages WHERE session_id='A'",[]).unwrap();assert!(validate_commit_with_connection(&c,"A",&a,3,true).is_err());}
 #[test]fn revision_and_crosssession_sameids_are_distinct(){let c=fixture();let a=resolve_with_connection(&c,"owner","A","session:A","same-id",3).unwrap();let b=resolve_with_connection(&c,"owner","B","session:B","same-id",3).unwrap();assert_ne!(a.boundary_hash,b.boundary_hash);assert!(validate_commit_with_connection(&c,"owner",&a,4,true).is_err());assert!(resolve_with_connection(&c,"owner","A","topic-label","same-id",3).is_err());}
 #[test]fn compact_original_boundary_is_part_of_exact_hash(){let c=fixture();let a=resolve_with_connection(&c,"A","A","session:A","same-id",3).unwrap();c.execute("UPDATE agent_messages SET compact_from_sequence=1 WHERE session_id='A'",[]).unwrap();assert!(validate_commit_with_connection(&c,"A",&a,3,true).is_err());}
}
