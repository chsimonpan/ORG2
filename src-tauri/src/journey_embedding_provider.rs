//! Production producer binding for the durable Journey embedding outbox.
//! Pins the exact space `memory_search` queries (model, 1024 dims, source fingerprint) and reuses its transport. No fallback model or provider.
use std::sync::Arc;
use async_trait::async_trait;
use agent_core::core::session::journey_embedding::{register_pinned_embedding,PinnedJourneyEmbedding};
use agent_core::integrations::config::EmbeddingConfig;
use agent_core::specialization::memory::embeddings::{EmbeddingProvider,EmbeddingResult};
use crate::memory_search_commands::{embed_text,MODEL,SOURCE};

type ConfigSource=Arc<dyn Fn()->EmbeddingConfig+Send+Sync>;
struct PinnedQwenEmbedding{config:ConfigSource}
#[async_trait]
impl EmbeddingProvider for PinnedQwenEmbedding{
 async fn embed(&self,text:&str)->Result<EmbeddingResult,String>{
  let vector=embed_text(text,&(self.config)()).await?;
  Ok(EmbeddingResult{dimensions:vector.len(),vector,model:MODEL.into()})
 }
 fn dimensions(&self)->usize{1024}
 fn provider_name(&self)->&str{"journey-pinned-zenmux-qwen3-vl-embedding"}
}
/// `config` is read at every embed so a native settings patch is honored; a mismatch fails the job explicitly.
pub(crate) fn register(config:ConfigSource)->Result<(),String>{
 register_pinned_embedding(PinnedJourneyEmbedding{provider:Arc::new(PinnedQwenEmbedding{config}),model:MODEL.into(),dimensions:1024,source:SOURCE.into()})
}

/// Test-only handle on the production provider type so tests exercise the same transport binding.
#[cfg(test)]
pub(crate) fn test_provider(config:ConfigSource)->impl EmbeddingProvider{PinnedQwenEmbedding{config}}
