//! Read-only owner-scoped persisted semantic search. No index creation or reembedding.
use serde::{Deserialize,Serialize};
use agent_core::specialization::memory::embeddings::rerank::ConfiguredReranker;
const MODEL:&str="qwen/qwen3-vl-embedding";
const SOURCE:&str="openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024";
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct MemorySearchHit { pub source_id:String,pub content:String,pub score:f32 }
#[tauri::command]
pub async fn memory_search(owner_session_id:String,query:String,query_vector:Option<Vec<f32>>,embedding_source:Option<String>,top_n:usize,state:tauri::State<'_,agent_core::state::AgentAppState>)->Result<Vec<MemorySearchHit>,String>{
 if owner_session_id.is_empty()||owner_session_id.starts_with("cli:")||state.get_session(&owner_session_id).await.is_none(){return Err("memory_owner_not_registered".into());}
 let query_vector=match query_vector { Some(v)=>{if embedding_source.as_deref()!=Some(SOURCE){return Err("memory_embedding_fingerprint_mismatch".into());}v},None=>embed_query(&query,&state).await? };
 if query_vector.len()!=1024||query_vector.iter().any(|x|!x.is_finite()){return Err("memory_embedding_fingerprint_mismatch".into());}
 if top_n>100{return Err("memory_search_limit".into());}
 let mut hits={
 let c=database::db::get_connection().map_err(|e|e.to_string())?;
 let mut s=c.prepare("SELECT source_id,content,embedding,embedding_model,embedding_source,embedding_dimensions FROM session_journey_embedding_index WHERE session_id=?1 ORDER BY source_kind,source_id").map_err(|e|e.to_string())?;
 let rows=s.query_map([&owner_session_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?))).map_err(|e|e.to_string())?;
 let mut hits=Vec::new();
 for row in rows{let(id,content,blob,model,source,dimensions)=row.map_err(|e|e.to_string())?;
 if model!=MODEL||source!=SOURCE||dimensions!=1024||blob.len()!=4096{return Err("memory_persisted_fingerprint_mismatch".into());}
 let vector:Vec<f32>=blob.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();
 if vector.iter().any(|x|!x.is_finite()){return Err("memory_nonfinite_vector".into());}
 let dot:f32=vector.iter().zip(&query_vector).map(|(a,b)|a*b).sum();
 let norm=(vector.iter().map(|x|x*x).sum::<f32>()*query_vector.iter().map(|x|x*x).sum::<f32>()).sqrt();
 if !norm.is_finite()||norm<=0.0{return Err("memory_invalid_vector_norm".into());}
 hits.push(MemorySearchHit{source_id:id,content,score:dot/norm});}
 hits};
 hits.sort_by(|a,b|b.score.total_cmp(&a.score));
 let reranker=ConfiguredReranker::from_config(state.integrations.snapshot().rerank)?;
 if !reranker.is_disabled(){let documents=hits.iter().map(|h|h.content.clone()).collect::<Vec<_>>();let ranked=reranker.rerank(&query,&documents,top_n).await?;hits=ranked.into_iter().map(|(i,score)|{let mut h=hits[i].clone();h.score=score;h}).collect();}
 hits.truncate(top_n);
 if state.get_session(&owner_session_id).await.is_none(){return Err("memory_owner_no_longer_registered".into());}
 Ok(hits)
}

/// Exact historical transport, independently validated from persisted synthetic fixtures.
async fn embed_query(query:&str,state:&agent_core::state::AgentAppState)->Result<Vec<f32>,String>{
 use key_vault::key_store::{KEY_SERVICE,HealthStatus};
 let config=state.integrations.snapshot().embedding;
 if config.provider!="embedding_api"||config.model.as_deref()!=Some(MODEL)||config.dimensions!=Some(1024){return Err("memory_query_config_fingerprint_mismatch".into());}
 if query.trim().is_empty(){return Err("memory_empty_query".into());}
 let mut keys=KEY_SERVICE.list_keys();keys.retain(|k| k.enabled&&k.health_status==HealthStatus::Valid&&k.enabled_models.iter().any(|m|m==MODEL)&&k.last_validated_at.is_some_and(|t|{let age=chrono::Utc::now().signed_duration_since(t).num_seconds();(0..=86400).contains(&age)}));keys.sort_by(|a,b|a.id.cmp(&b.id));
 let key=keys.into_iter().find(|k| k.base_url.as_deref().is_some_and(allowed_embedding_base)).ok_or("memory_query_credential_unavailable")?;
 let base=key.base_url.as_deref().ok_or("memory_query_base_unavailable")?;
 let secret=key.api_key.as_deref().filter(|k|!k.trim().is_empty()).ok_or("memory_query_credential_unavailable")?;
 let response=reqwest::Client::new().post(format!("{}/embeddings",base.trim_end_matches('/'))).bearer_auth(secret).json(&serde_json::json!({"model":MODEL,"input":[query],"dimensions":1024})).send().await.map_err(|_|"memory_query_transport_failed")?;
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
