//! Read-only owner-scoped persisted semantic search. No index creation or reembedding.
//! Each hit carries a same-owner durable source anchor resolved from the Journey aggregate, or an explicit unavailable reason.
use serde::{Deserialize,Serialize};
use agent_core::specialization::memory::embeddings::rerank::ConfiguredReranker;
use agent_core::core::session::journey_canonical::{DurableAnchor,resolve_with_connection};
use agent_core::core::session::journey_lifecycle::{SessionJourney,SqliteJourneyRepository};
pub(crate) const MODEL:&str="qwen/qwen3-vl-embedding";
pub(crate) const SOURCE:&str="openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024";
/// Hard ceiling on index rows scanned per query. Above it the query fails explicitly (`memory_search_candidate_limit`); it never silently searches a subset.
pub(crate) const MAX_CANDIDATES:usize=2000;
/// Only the cosine top-K reach the rerank provider (K = the `top_n` ceiling), bounding provider payload and cost.
pub(crate) const MAX_RERANK_CANDIDATES:usize=100;
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct MemorySearchHit { pub source_id:String,pub content:String,pub score:f32,pub source_kind:String,pub fork_id:String,pub review_id:String,
 /// Same-owner durable message boundary of the evidence; `None` always comes with `source_unavailable`.
 pub source_anchor:Option<DurableAnchor>,pub source_unavailable:Option<String> }
async fn require_registered_owner(owner:&str,state:&agent_core::state::AgentAppState)->Result<(),String>{
 if owner.is_empty()||owner.starts_with("cli:")||state.get_session(owner).await.is_none(){return Err("memory_owner_not_registered".into());}Ok(())
}
/// Owner-scoped embedding pipeline state (queued/running/indexed and failed jobs with their reason), so a failed or exhausted index is never an invisible empty search.
#[tauri::command]
pub async fn memory_index_status(owner_session_id:String,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<agent_core::core::session::journey_embedding::JourneyEmbeddingStatus,String>{
 require_registered_owner(&owner_session_id,&state).await?;
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 agent_core::core::session::journey_embedding::status_for_owner(&c,&owner_session_id).map_err(|e|e.to_string())
}
#[tauri::command]
pub async fn memory_search(owner_session_id:String,query:String,query_vector:Option<Vec<f32>>,embedding_source:Option<String>,top_n:usize,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<Vec<MemorySearchHit>,String>{
 require_registered_owner(&owner_session_id,&state).await?;
 let query_vector=match query_vector { Some(v)=>{if embedding_source.as_deref()!=Some(SOURCE){return Err("memory_embedding_fingerprint_mismatch".into());}v},None=>embed_query(&query,&state).await? };
 if query_vector.len()!=1024||query_vector.iter().any(|x|!x.is_finite()){return Err("memory_embedding_fingerprint_mismatch".into());}
 if top_n>100{return Err("memory_search_limit".into());}
 let mut hits={
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 let mut s=c.prepare("SELECT source_id,content,embedding,embedding_model,embedding_source,embedding_dimensions,source_kind,fork_id,review_id FROM session_journey_embedding_index WHERE session_id=?1 ORDER BY source_kind,source_id LIMIT ?2").map_err(|e|e.to_string())?;
 let rows=s.query_map(rusqlite::params![&owner_session_id,(MAX_CANDIDATES+1) as i64],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?))).map_err(|e|e.to_string())?;
 let mut hits=Vec::new();
 for row in rows{if hits.len()>=MAX_CANDIDATES{return Err("memory_search_candidate_limit".into());}
 let(id,content,blob,model,source,dimensions,kind,fork,review)=row.map_err(|e|e.to_string())?;
 if model!=MODEL||source!=SOURCE||dimensions!=1024||blob.len()!=4096{return Err("memory_persisted_fingerprint_mismatch".into());}
 let vector:Vec<f32>=blob.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();
 if vector.iter().any(|x|!x.is_finite()){return Err("memory_nonfinite_vector".into());}
 let dot:f32=vector.iter().zip(&query_vector).map(|(a,b)|a*b).sum();
 let norm=(vector.iter().map(|x|x*x).sum::<f32>()*query_vector.iter().map(|x|x*x).sum::<f32>()).sqrt();
 if !norm.is_finite()||norm<=0.0{return Err("memory_invalid_vector_norm".into());}
 hits.push(MemorySearchHit{source_id:id,content,score:dot/norm,source_kind:kind,fork_id:fork,review_id:review,source_anchor:None,source_unavailable:None});}
 hits};
 hits.sort_by(|a,b|b.score.total_cmp(&a.score));
 let reranker=ConfiguredReranker::from_config(state.integrations.snapshot().rerank)?;
 if !reranker.is_disabled(){hits.truncate(MAX_RERANK_CANDIDATES);let documents=hits.iter().map(|h|h.content.clone()).collect::<Vec<_>>();let ranked=reranker.rerank(&query,&documents,top_n).await?;hits=ranked.into_iter().map(|(i,score)|{let mut h=hits[i].clone();h.score=score;h}).collect();}
 hits.truncate(top_n);
 attach_sources(&owner_session_id,&mut hits)?;
 if state.get_session(&owner_session_id).await.is_none(){return Err("memory_owner_no_longer_registered".into());}
 Ok(hits)
}

fn attach_sources(owner:&str,hits:&mut [MemorySearchHit])->Result<(),String>{
 if hits.is_empty(){return Ok(());}
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 let journey=SqliteJourneyRepository::load_existing(&c,owner).map_err(|e|format!("memory_source_journey_unreadable:{e:?}"))?;
 let revision:i64=c.query_row("SELECT COALESCE(MAX(sequence),0) FROM agent_messages WHERE session_id=?1",[owner],|r|r.get(0)).map_err(|e|e.to_string())?;
 for h in hits.iter_mut(){match resolve_source(&c,owner,journey.as_ref(),revision.max(0) as u64,h){Ok(a)=>h.source_anchor=Some(a),Err(reason)=>h.source_unavailable=Some(reason.into())}}
 Ok(())
}
/// Index identity must agree with the owner's own Journey aggregate; any disagreement is an explicit unavailable source, never a guessed one.
fn resolve_source(c:&rusqlite::Connection,owner:&str,journey:Option<&SessionJourney>,revision:u64,h:&MemorySearchHit)->Result<DurableAnchor,&'static str>{
 let journey=journey.ok_or("source_journey_missing")?;
 let message_id:String=match h.source_kind.as_str(){
  "confirmed_fact"=>{let id=h.source_id.strip_prefix("fact:").ok_or("source_id_unrecognized")?;let f=journey.facts.get(id).ok_or("source_fact_missing")?;
   if f.review_id!=h.review_id||f.text!=h.content{return Err("source_fact_mismatch");}f.evidence_end_message_id.clone()}
  "review_annotation"=>{let id=h.source_id.strip_prefix("review:").ok_or("source_id_unrecognized")?;let r=journey.reviews.get(id).ok_or("source_review_missing")?;
   if r.id!=h.review_id||r.fork_id!=h.fork_id||r.annotation.as_deref()!=Some(h.content.as_str()){return Err("source_review_mismatch");}
   c.query_row("SELECT id FROM agent_messages WHERE session_id=?1 AND sequence=?2 ORDER BY id LIMIT 1",rusqlite::params![owner,r.source_end_sequence as i64],|row|row.get::<_,String>(0)).map_err(|_|"source_message_missing")?}
  _=>return Err("source_kind_unrecognized")};
 resolve_with_connection(c,owner,owner,&format!("session:{owner}"),&message_id,revision).map_err(|_|"source_anchor_unresolvable")
}

/// Exact historical transport, independently validated from persisted synthetic fixtures.
async fn embed_query(query:&str,state:&agent_core::state::AgentAppState)->Result<Vec<f32>,String>{
 embed_text(query,&state.integrations.snapshot().embedding).await
}
/// Single embedding transport shared by query-time search and the durable index producer, so both live in one vector space.
pub(crate) async fn embed_text(query:&str,config:&agent_core::integrations::config::EmbeddingConfig)->Result<Vec<f32>,String>{
 use key_vault::key_store::{KEY_SERVICE,HealthStatus};
 if config.provider!="embedding_api"||config.model.as_deref()!=Some(MODEL)||config.dimensions!=Some(1024){return Err("memory_query_config_fingerprint_mismatch".into());}
 if query.trim().is_empty(){return Err("memory_empty_query".into());}
 let mut keys=KEY_SERVICE.list_keys();keys.retain(|k| k.enabled&&k.health_status==HealthStatus::Valid&&k.enabled_models.iter().any(|m|m==MODEL)&&k.last_validated_at.is_some_and(|t|{let age=chrono::Utc::now().signed_duration_since(t).num_seconds();(0..=86400).contains(&age)}));keys.sort_by(|a,b|a.id.cmp(&b.id));
 let key=keys.into_iter().find(|k| k.base_url.as_deref().is_some_and(allowed_embedding_base)).ok_or("memory_query_credential_unavailable")?;
 let base=key.base_url.as_deref().ok_or("memory_query_base_unavailable")?;
 let secret=key.api_key.as_deref().filter(|k|!k.trim().is_empty()).ok_or("memory_query_credential_unavailable")?;
 let response=reqwest::Client::new().post(format!("{}/embeddings",base.trim_end_matches('/'))).bearer_auth(secret).json(&serde_json::json!({"model":MODEL,"input":query,"encoding_format":"float","dimensions":1024})).send().await.map_err(|_|"memory_query_transport_failed")?;
 if !response.status().is_success(){return Err("memory_query_transport_status".into());}
 let body:serde_json::Value=response.json().await.map_err(|_|"memory_query_response_invalid")?;
 let rows=body["data"].as_array().ok_or("memory_query_response_invalid")?;if rows.len()!=1{return Err("memory_query_response_invalid".into());}
 let values=rows[0]["embedding"].as_array().ok_or("memory_query_response_invalid")?;if values.len()!=1024{return Err("memory_query_dimension_mismatch".into());}
 let vector:Vec<f32>=values.iter().map(|v|v.as_f64().map(|v|v as f32).filter(|v|v.is_finite()).ok_or("memory_query_nonfinite")).collect::<Result<_,_>>()?;
 Ok(vector)
}
fn allowed_embedding_base(base:&str)->bool{
 if base=="https://zenmux.ai/api/v1"{return true;}
 // Test-only loopback protocol mock; not a runtime fallback or alternate space.
 #[cfg(test)] {return base.starts_with("http://127.0.0.1:")&&base.ends_with("/api/v1");}
 #[cfg(not(test))] false
}
