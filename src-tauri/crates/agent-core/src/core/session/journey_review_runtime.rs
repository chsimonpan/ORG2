//! Production provider wiring for the Journey review executor.
//!
//! The provider is rebuilt per job from the owner session's persisted runtime (model, exactly one
//! credential owner, harness). Its provenance must equal the one pinned on the job; a mismatch is
//! a visible failure, never a substitution.
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::core::session::journey_lifecycle::RuntimeProvenance;
use crate::core::session::journey_review_queue::{
    JourneyReviewProviderResolver, JourneyUnboundedReviewProvider, REVIEW_OUTPUT_CEILING,
};
use crate::core::session::task_fork::provider_from_persisted_config;
use crate::providers::traits::{LLMProvider, LLMResponse};

pub fn provenance_of(
    provider: &dyn LLMProvider,
    model: &str,
    account_id: Option<&str>,
) -> Result<RuntimeProvenance, String> {
    let account = account_id
        .map(str::to_string)
        .or_else(|| provider.credential_source().map(str::to_string))
        .unwrap_or_default();
    let provenance = RuntimeProvenance {
        model_id: model.to_string(),
        account_id: account,
        protocol: provider.provider_name().to_string(),
    };
    if provenance.model_id.trim().is_empty()
        || provenance.account_id.trim().is_empty()
        || provenance.protocol.trim().is_empty()
    {
        return Err("journey_review_runtime_provenance_missing".into());
    }
    Ok(provenance)
}

/// Provenance the owner session would run a review with right now (used when closing a fork).
pub async fn review_provenance_from_persisted_config(
    session_id: &str,
) -> Result<RuntimeProvenance, String> {
    let (provider, model, account) = provider_from_persisted_config(session_id).await?;
    provenance_of(provider.as_ref(), &model, account.as_deref())
}

struct CeilingReviewProvider {
    provider: Arc<dyn LLMProvider>,
}

#[async_trait]
impl JourneyUnboundedReviewProvider for CeilingReviewProvider {
    async fn review_unbounded(
        &self,
        messages: &[Value],
        tools: &[Value],
        model: &str,
        temperature: f32,
    ) -> Result<LLMResponse, String> {
        // Streaming keeps the 64k ceiling legal for providers that refuse long non-streaming calls.
        self.provider
            .chat_streaming(messages, Some(tools), model, REVIEW_OUTPUT_CEILING, temperature, &|_| {}, None)
            .await
            .map_err(|error| error.to_string())
    }
}

pub struct PersistedConfigReviewResolver;

#[async_trait]
impl JourneyReviewProviderResolver for PersistedConfigReviewResolver {
    async fn resolve(
        &self,
        session_id: &str,
        expected: &RuntimeProvenance,
    ) -> Result<Arc<dyn JourneyUnboundedReviewProvider>, String> {
        let (provider, model, account) = provider_from_persisted_config(session_id).await?;
        let actual = provenance_of(provider.as_ref(), &model, account.as_deref())?;
        if &actual != expected {
            return Err(format!(
                "The session now resolves to {}/{}/{} but the review was pinned to {}/{}/{}; refusing to substitute.",
                actual.model_id, actual.account_id, actual.protocol,
                expected.model_id, expected.account_id, expected.protocol
            ));
        }
        Ok(Arc::new(CeilingReviewProvider { provider }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::ProviderError;
    use std::sync::Mutex;

    struct Recorder {
        max_tokens: Mutex<Vec<u32>>,
    }

    #[async_trait]
    impl LLMProvider for Recorder {
        async fn chat(
            &self,
            _m: &[Value],
            _t: Option<&[Value]>,
            _model: &str,
            max_tokens: u32,
            _temperature: f32,
        ) -> Result<LLMResponse, ProviderError> {
            self.max_tokens.lock().unwrap().push(max_tokens);
            Ok(LLMResponse::text("ok"))
        }
        fn default_model(&self) -> &str {
            "m"
        }
        fn provider_name(&self) -> &str {
            "recorder"
        }
        fn credential_source(&self) -> Option<&str> {
            Some("src")
        }
    }

    #[test]
    fn provenance_uses_account_else_credential_source_and_rejects_empty() {
        let p = Recorder { max_tokens: Mutex::new(vec![]) };
        let a = provenance_of(&p, "model-x", Some("acct")).unwrap();
        assert_eq!((a.model_id.as_str(), a.account_id.as_str(), a.protocol.as_str()), ("model-x", "acct", "recorder"));
        assert_eq!(provenance_of(&p, "model-x", None).unwrap().account_id, "src");
        assert!(provenance_of(&p, " ", Some("acct")).is_err());
    }

    #[tokio::test]
    async fn review_adapter_sends_the_model_ceiling() {
        let recorder = Arc::new(Recorder { max_tokens: Mutex::new(vec![]) });
        let adapter = CeilingReviewProvider { provider: recorder.clone() };
        adapter.review_unbounded(&[], &[], "m", 0.0).await.unwrap();
        assert_eq!(*recorder.max_tokens.lock().unwrap(), vec![REVIEW_OUTPUT_CEILING]);
        assert_eq!(REVIEW_OUTPUT_CEILING, 64_000);
    }
}
