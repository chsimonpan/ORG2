//! Production producer binding for the durable Journey embedding outbox.
//! Pins the exact space `memory_search` queries (model, 1024 dims, source fingerprint) and reuses its transport. No fallback model or provider.
use std::sync::Arc;
use async_trait::async_trait;
use agent_core::core::session::journey_embedding::{register_pinned_embedding,PinnedJourneyEmbedding};
use agent_core::integrations::config::EmbeddingConfig;
use agent_core::specialization::memory::embeddings::{EmbeddingProvider,EmbeddingResult};
use crate::memory_search_commands::{embed_text,configured_space};

type ConfigSource=Arc<dyn Fn()->EmbeddingConfig+Send+Sync>;
struct PinnedQwenEmbedding{config:ConfigSource,model:String,dims:usize,source:String}
impl PinnedQwenEmbedding{
 fn pin(config:ConfigSource)->Self{let (model,dims,source)=configured_space(&config()).unwrap_or_else(|_|(String::new(),0,String::new()));Self{config,model,dims,source}}
}
#[async_trait]
impl EmbeddingProvider for PinnedQwenEmbedding{
 async fn embed(&self,text:&str)->Result<EmbeddingResult,String>{
  let cfg=(self.config)();
  // The pin is taken at startup; a settings change to another space must fail explicitly instead of mixing vectors.
  if configured_space(&cfg)?.2!=self.source{return Err("memory_query_config_fingerprint_mismatch".into());}
  let vector=embed_text(text,&cfg).await?;
  Ok(EmbeddingResult{dimensions:vector.len(),vector,model:self.model.clone()})
 }
 fn dimensions(&self)->usize{self.dims}
 fn provider_name(&self)->&str{"journey-pinned-configured-embedding"}
}
/// `config` is read at every embed so a native settings patch is honored; a mismatch fails the job explicitly.
pub(crate) fn register(config:ConfigSource)->Result<(),String>{
 let (model,dims,source)=configured_space(&config())?;
 register_pinned_embedding(PinnedJourneyEmbedding{provider:Arc::new(PinnedQwenEmbedding{config,model:model.clone(),dims,source:source.clone()}),model,dimensions:dims,source})
}

/// Test-only handle on the production provider type so tests exercise the same transport binding.
#[cfg(test)]
pub(crate) fn test_provider(config:ConfigSource)->impl EmbeddingProvider{
 use crate::memory_search_commands::{MODEL,SOURCE};
 // Pinned to the default space explicitly, so a later config change is observed as drift rather than re-pinned.
 PinnedQwenEmbedding{config,model:MODEL.into(),dims:1024,source:SOURCE.into()}
}
