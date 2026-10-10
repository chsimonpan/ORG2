//! Content-free monotonic observations; provider_call includes adapter work, not pure network.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
static NEXT: AtomicU64 = AtomicU64::new(1);
tokio::task_local! { static REQUEST: u64; }
pub(crate) fn next_request() -> u64 {
    REQUEST.try_with(|id| *id).unwrap_or_else(|_| NEXT.fetch_add(1, Ordering::Relaxed))
}
pub(crate) async fn correlated<F: std::future::Future>(request: u64, future: F) -> F::Output {
    REQUEST.scope(request, future).await
}
pub(crate) struct Phase { request: u64, phase: &'static str, start: Instant, done: bool }
impl Phase {
    pub(crate) fn new(request: u64, phase: &'static str) -> Self {
        tracing::info!(request, phase, status = "started", "[compaction_phase]");
        Self { request, phase, start: Instant::now(), done: false }
    }
    pub(crate) fn finish(mut self, status: &'static str) { self.emit(status); self.done = true; }
    fn emit(&self, status: &'static str) {
        tracing::info!(request = self.request, phase = self.phase, status,
            elapsed_us = self.start.elapsed().as_micros() as u64, "[compaction_phase]");
    }
}
impl Drop for Phase { fn drop(&mut self) { if !self.done { self.emit("interrupted"); } } }

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::instrument::WithSubscriber;
    #[tokio::test]
    async fn compaction_phase_real_delay_lock_wait_and_cancel() {
        let request = next_request();
        let capture = capture::Capture::default();
        correlated(request, async {
            assert_eq!(next_request(), request);
            let phase = Phase::new(request, "provider_call");
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            assert!(phase.start.elapsed().as_millis() >= 15);
            phase.finish("error");
            let lock = tokio::sync::Mutex::new(());
            let held = lock.lock().await;
            let mut waiter = Box::pin(async {
                let phase = Phase::new(next_request(), "state_lock_wait");
                let _held = lock.lock().await;
                phase.finish("ok");
            });
            tokio::select! {
                _ = &mut waiter => panic!("held lock unexpectedly acquired"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(15)) => {}
            }
            drop(waiter);
            drop(held);
        }).with_subscriber(capture.clone()).await;
        assert_eq!(capture.assert_phase("state_lock_wait", "interrupted"), request);
        assert_eq!(capture.assert_phase("provider_call", "error"), request);
        let rows = capture.0.lock().unwrap();
        let wait = rows.iter().find(|r| r.get("phase") == Some(&"\"state_lock_wait\"".into()) && r.contains_key("elapsed_us")).unwrap();
        assert!(wait["elapsed_us"].parse::<u64>().unwrap() >= 15_000);
    }
}

#[cfg(test)]
pub(crate) mod capture {
    use std::sync::{Arc, Mutex};
    use tracing::{Subscriber, Event, Metadata, span::{Attributes, Record, Id}, field::{Visit, Field}};
    #[derive(Clone, Default)]
    pub(crate) struct Capture(pub Arc<Mutex<Vec<std::collections::BTreeMap<String, String>>>>);
    struct Fields(std::collections::BTreeMap<String, String>);
    impl Visit for Fields {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().into(), format!("{value:?}"));
        }
    }
    impl Subscriber for Capture {
        fn enabled(&self, _: &Metadata<'_>) -> bool { true }
        fn new_span(&self, _: &Attributes<'_>) -> Id { Id::from_u64(1) }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut fields = Fields(Default::default()); event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }
    impl Capture {
        pub(crate) fn assert_phase(&self, phase: &str, status: &str) -> u64 {
            let rows = self.0.lock().unwrap();
            let row = rows.iter().find(|r| r.get("phase") == Some(&format!("{phase:?}")) && r.get("status") == Some(&format!("{status:?}")))
                .unwrap_or_else(|| panic!("missing {phase}/{status}: {rows:?}"));
            let request = row["request"].parse().unwrap();
            assert!(rows.iter().any(|r| r.get("request") == row.get("request") && r.get("phase") == row.get("phase") && r.get("status") == Some(&"\"started\"".into())));
            assert!(row["elapsed_us"].parse::<u64>().is_ok()); request
        }
        pub(crate) fn assert_secret_absent(&self) {
            assert!(!format!("{:?}", self.0.lock().unwrap()).contains("TRACE_SECRET"));
        }
    }
}

#[cfg(test)]
mod capture_contract {
    use super::*;
    use tracing::instrument::WithSubscriber;
    use crate::providers::traits::{LLMProvider, LLMResponse, ProviderError};
    struct Provider { calls: std::sync::atomic::AtomicUsize, fail: bool, pending: bool }
    #[async_trait::async_trait]
    impl LLMProvider for Provider {
        async fn chat(&self, _: &[serde_json::Value], _: Option<&[serde_json::Value]>, _: &str, _: u32, _: f32) -> Result<LLMResponse, ProviderError> {
            if self.pending { std::future::pending::<()>().await; }
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            if self.fail { return Err(ProviderError::RequestFailed("fixture failure".into())); }
            let first = self.calls.fetch_add(1, Ordering::Relaxed) == 0;
            Ok(LLMResponse {content: Some("TRACE_SECRET_REPLY".into()), tool_calls: vec![], finish_reason: if first {"length"} else {"stop"}.into(), usage: Default::default(), reasoning_content: None, blocks: vec![], stream_error_kind: None, retry_after_ms: None})
        }
        fn default_model(&self) -> &str { "fixture-model" }
        fn provider_name(&self) -> &str { "fixture" }
    }
    #[tokio::test(flavor="multi_thread", worker_threads=2)]
    async fn compaction_capture_production_adapter_retry_error_cancel() {
        let capture = capture::Capture::default();
        let request = next_request();
        let config = crate::core::side_query::SideQueryConfig { system_prompt: Some("TRACE_SECRET_PROMPT".into()), ..Default::default() };
        let messages = vec![serde_json::json!({"role":"user","content":"TRACE_SECRET_MESSAGE"})];
        correlated(request, async {
            let provider = Provider {calls: Default::default(), fail:false, pending:false};
            crate::core::side_query::side_query(&provider, &messages, &config, "fixture-model").await.unwrap();
            for phase in ["side_query_adapter_call", "side_query_retry_adapter_call", "side_query_retry_parse"] {
                assert_eq!(capture.assert_phase(phase,"ok"), request);
            }
            assert_eq!(capture.assert_phase("side_query_parse","failed_or_retry"),request);
            let rows = capture.0.lock().unwrap();
            let elapsed = |phase: &str| rows.iter().find(|r| r.get("phase")==Some(&format!("{phase:?}")) && r.get("status")==Some(&"\"ok\"".into())).unwrap()["elapsed_us"].parse::<u64>().unwrap();
            assert!(elapsed("side_query_adapter_call") >= 15_000);
            assert!(elapsed("side_query_retry_adapter_call") >= 15_000);
            assert!(elapsed("side_query_retry_parse") < elapsed("side_query_retry_adapter_call"));
            drop(rows);
            let provider = Provider {calls: Default::default(), fail:true, pending:false};
            assert!(crate::core::side_query::side_query(&provider,&messages,&config,"fixture-model").await.is_err());
            assert_eq!(capture.assert_phase("side_query_adapter_call","failed"),request);
            assert_eq!(capture.assert_phase("side_query_parse","provider_failed"),request);
            let provider = Provider {calls: Default::default(), fail:false, pending:true};
            let mut call = Box::pin(crate::core::side_query::side_query(&provider,&messages,&config,"fixture-model"));
            tokio::select! { _ = &mut call => panic!("pending provider completed"), _ = tokio::time::sleep(std::time::Duration::from_millis(15)) => {} }
            drop(call);
            assert_eq!(capture.assert_phase("side_query_adapter_call","interrupted"),request);
        }).with_subscriber(capture.clone()).await;
        capture.assert_secret_absent();
    }
}
