//! Exercises reactive compaction and the real durable summary commit together.
use super::*;
use crate::definitions::{resolved::ResolvedAgent, AgentDefinition};
use crate::memory::background::MemoryJobCompletion;
use crate::providers::{
    auxiliary_model::AuxiliaryModel,
    traits::{LLMProvider, LLMResponse, ProviderError},
};
use crate::session::turn::post_turn::extract_and_persist_session_memory;
use crate::state::{AgentSession, SessionRuntime};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct FixtureProvider {
    calls: AtomicUsize,
    fail: bool,
}
#[async_trait]
impl LLMProvider for FixtureProvider {
    async fn chat(
        &self,
        _: &[Value],
        _: Option<&[Value]>,
        _: &str,
        _: u32,
        _: f32,
    ) -> Result<LLMResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(ProviderError::RequestFailed(
                "injected compaction failure".into(),
            ))
        } else {
            Ok(LLMResponse::text(
                "### Current State\nNew committed summary",
            ))
        }
    }
    fn default_model(&self) -> &str {
        "test/model"
    }
    fn provider_name(&self) -> &str {
        "reactive-fixture"
    }
    fn auxiliary_model(&self, _: &str) -> AuxiliaryModel {
        AuxiliaryModel {
            models: vec!["test/cheap".into()],
            scope: 17,
        }
    }
}

fn fixture(sid: &str, fail: bool) -> (UnifiedMessageProcessor, Arc<FixtureProvider>) {
    let provider = Arc::new(FixtureProvider {
        calls: AtomicUsize::new(0),
        fail,
    });
    (fixture_with(sid, provider.clone()), provider)
}

fn fixture_with(sid: &str, provider: Arc<dyn LLMProvider>) -> UnifiedMessageProcessor {
    let conn = database::db::get_connection().unwrap();
    crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
    crate::persistence::session_snapshots::ensure_tables_with(&conn).unwrap();
    conn.execute("INSERT INTO agent_sessions (session_id,name,session_type,status,created_at,updated_at) VALUES (?1,'Reactive fixture','agent','running',datetime('now'),datetime('now'))", [sid]).unwrap();
    for i in 0..4 {
        unified_persistence::save_user_msg(
            sid,
            &format!("turn {i}: {}", "work ".repeat(100)),
            None,
        )
        .unwrap();
        unified_persistence::save_assistant_msg(sid, "done", "test/model").unwrap();
    }
    unified_persistence::save_session_memory_state(sid, "old memory", Some(3)).unwrap();
    let def = AgentDefinition {
        selected_model_id: Some("test/model".into()),
        ..Default::default()
    };
    let mut resolved = ResolvedAgent::resolve(&def, None, &Default::default()).unwrap();
    resolved.context_window = 100_000;
    resolved.context_window_configured = true;
    let session = Arc::new(AgentSession::new(sid.into(), def));
    let policy = Arc::new(crate::tools::policy::ResolvedToolPolicy::permissive());
    let runtime = Arc::new(SessionRuntime {
        provider: provider.clone(),
        tool_registry: Arc::new(crate::tools::registry::ToolRegistry::new()),
        policy: policy.clone(),
        model: "test/model".into(),
        account_id: None,
        native_harness_type: None,
        workspace_state: Arc::new(parking_lot::RwLock::new(
            crate::session::workspace::SessionWorkspace::new(std::env::temp_dir()),
        )),
        mcp_auto_approved: vec![],
        resolved,
        integrations_snapshot: Default::default(),
        overrides: Default::default(),
        agent_soul: None,
        sovereign_prompt: false,
        policy_context_activator: None,
        agent_org_context: None,
        agent_org_current_member_id: None,
        agent_definition_id: None,
    });
    let mut processor = UnifiedMessageProcessor::new(super::super::ProcessorParams {
        runtime,
        session,
        policy,
        channel: None,
        chat_id: None,
        agent_mode: None,
        ide_context: None,
        app_handle: None,
        screenshot_store: Arc::new(Default::default()),
        event_handler_config: Default::default(),
    });
    processor.sm_compact_config.min_tokens_to_keep = 0;
    processor.sm_compact_config.min_text_messages_to_keep = 1;
    processor
}

#[tokio::test(flavor = "multi_thread")]
async fn session_memory_reactive_compaction_allows_new_durable_commit() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "reactive-memory-cas";
    let (processor, provider) = fixture(sid, false);
    {
        let mut state = processor.sm_state.lock().await;
        state.content = Some("old memory".into());
        state.last_summarized_seq = Some(3);
        state.initialized = true;
    }
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    let outcome = processor
        .run_reactive_compaction(sid, &mut messages, None)
        .await;
    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "SM compaction is local"
    );
    assert_eq!(processor.sm_state.lock().await.last_summarized_seq, None);
    assert_eq!(
        unified_persistence::load_session_memory_state(sid)
            .unwrap()
            .last_seq,
        Some(3)
    );
    // Each view is current at extraction start: runtime anchor was reset,
    // while reactive compaction deliberately did not write a durable boundary.
    for tokens in [25_000, 31_000] {
        assert_eq!(
            extract_and_persist_session_memory(
                sid,
                processor.sm_state.clone(),
                &processor.sm_config,
                provider.as_ref(),
                "test/model",
                tokens,
                None
            )
            .await
            .unwrap(),
            MemoryJobCompletion::Completed
        );
        let durable = unified_persistence::load_session_memory_state(sid).unwrap();
        let state = processor.sm_state.lock().await;
        assert_eq!(state.content, durable.content);
        assert_eq!(state.last_summarized_seq, durable.last_seq);
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_memory_failed_reactive_compaction_preserves_growth() {
    let _sandbox = test_helpers::test_env::sandbox();
    for sm_enabled in [false, true] {
        let sid = format!("reactive-failed-{sm_enabled}");
        let (mut processor, provider) = fixture(&sid, true);
        processor.sm_config.enabled = sm_enabled;
        Arc::get_mut(&mut processor.runtime)
            .unwrap()
            .resolved
            .compaction
            .min_messages = 1;
        {
            let mut state = processor.sm_state.lock().await;
            state.content = Some("old memory".into());
            state.last_summarized_seq = Some(3);
            state.initialized = true;
        }
        assert!(
            crate::session::turn::post_turn::prepare_session_memory_baseline(
                &sid,
                processor.sm_state.clone(),
                20_000
            )
            .await
        );
        {
            let mut state = processor.sm_state.lock().await;
            state.extraction_generation = 7;
            state.tool_calls_since_extraction = 8;
        }
        processor
            .session
            .last_context_tokens
            .store(45_000, Ordering::SeqCst);
        let original: Vec<Value> = (0..8).map(|i| serde_json::json!({
            "role": "user", "content": if i == 7 { "recent tail".to_owned() } else { "work ".repeat(40_000) }
        })).collect();
        let mut messages = original.clone();
        let outcome = processor
            .run_reactive_compaction(&sid, &mut messages, None)
            .await;
        assert!(
            matches!(outcome, CompactionOutcome::Failed { .. }),
            "{outcome:?}"
        );
        assert!(
            provider.calls.load(Ordering::SeqCst) >= 2,
            "normal and rescue must run"
        );
        assert_eq!(
            messages, original,
            "failed compaction must preserve the original frame"
        );
        {
            let state = processor.sm_state.lock().await;
            assert_eq!(state.tokens_at_last_extraction, Some(20_000));
            assert_eq!(state.last_summarized_seq, Some(3));
            assert_eq!(state.extraction_generation, 7);
            assert_eq!(state.tool_calls_since_extraction, 8);
        }
        assert_eq!(
            processor.session.last_context_tokens.load(Ordering::SeqCst),
            45_000
        );
        assert!(
            crate::session::turn::post_turn::prepare_session_memory_baseline(
                &sid,
                processor.sm_state.clone(),
                45_000
            )
            .await
        );
        let durable = unified_persistence::load_session_memory_state(&sid).unwrap();
        assert_eq!(durable.tokens_at_last_extraction, Some(20_000));
        assert_eq!(durable.last_seq, Some(3));
        assert_eq!(durable.content.as_deref(), Some("old memory"));
        processor.sm_config.enabled = true;
        let state = processor.sm_state.lock().await;
        assert_eq!(state.tokens_at_last_extraction, Some(20_000));
        assert!(session_memory::should_extract(
            &state,
            &processor.sm_config,
            45_000,
            false
        ));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn session_memory_skipped_reactive_compaction_preserves_growth() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "reactive-skipped-memory";
    let (mut processor, provider) = fixture(sid, false);
    processor.sm_config.enabled = false;
    {
        let mut state = processor.sm_state.lock().await;
        state.content = Some("old memory".into());
        state.last_summarized_seq = Some(3);
        state.tokens_at_last_extraction = Some(20_000);
        state.initialized = true;
        state.extraction_generation = 7;
    }
    processor
        .session
        .last_context_tokens
        .store(45_000, Ordering::SeqCst);
    let original = unified_persistence::load_llm_history(sid).unwrap();
    let mut messages = original.clone();
    assert!(matches!(
        processor
            .run_reactive_compaction(sid, &mut messages, None)
            .await,
        CompactionOutcome::Skipped
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(messages, original);
    let state = processor.sm_state.lock().await;
    assert_eq!(state.tokens_at_last_extraction, Some(20_000));
    assert_eq!(state.last_summarized_seq, Some(3));
    assert_eq!(state.extraction_generation, 7);
    assert_eq!(
        processor.session.last_context_tokens.load(Ordering::SeqCst),
        45_000
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn session_memory_pre_turn_skipped_compaction_preserves_baseline() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pre-turn-skipped-memory";
    let (mut processor, provider) = fixture(sid, false);
    processor.sm_config.enabled = false;
    {
        let mut state = processor.sm_state.lock().await;
        state.content = Some("old memory".into());
        state.last_summarized_seq = Some(3);
        state.tokens_at_last_extraction = Some(20_000);
        state.initialized = true;
        state.extraction_generation = 7;
    }
    // A stale provider observation opens the gate, but there is no current
    // compactable tail. The compactor returns Skipped without an LLM call.
    processor
        .session
        .last_context_tokens
        .store(100_000, Ordering::SeqCst);
    let mut messages = Vec::new();
    assert!(matches!(
        processor.run_pre_turn_compaction(sid, &mut messages).await,
        CompactionPhaseOutcome::Continue
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(messages.is_empty());
    let state = processor.sm_state.lock().await;
    assert_eq!(state.tokens_at_last_extraction, Some(20_000));
    assert_eq!(state.last_summarized_seq, Some(3));
    assert_eq!(state.extraction_generation, 7);
    assert_eq!(
        processor.session.last_context_tokens.load(Ordering::SeqCst),
        100_000
    );
    assert_eq!(unified_persistence::load_llm_history(sid).unwrap().len(), 8);
}

/// Summary provider that parks inside the request until released; no network.
struct GatedProvider {
    calls: AtomicUsize,
    gate: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[async_trait]
impl LLMProvider for GatedProvider {
    async fn chat(
        &self,
        _: &[Value],
        _: Option<&[Value]>,
        _: &str,
        _: u32,
        _: f32,
    ) -> Result<LLMResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.gate.load(Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(LLMResponse::text("### Current State\nGated committed summary"))
    }
    fn default_model(&self) -> &str {
        "test/model"
    }
    fn provider_name(&self) -> &str {
        "gated-fixture"
    }
}

/// Spend-triggered pre-turn config shared by the R1/R4 baseline tests.
fn arm_weighted_pre_turn(processor: &mut UnifiedMessageProcessor, sid: &str) {
    // Same shape as the real-Wry pre-turn fixtures: LLM path only, small window.
    processor.sm_config.enabled = false;
    let resolved = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved;
    resolved.context_window = 1000;
    let config = &mut resolved.compaction;
    config.enabled = true;
    config.weighted_token_threshold = 1;
    config.trigger_ratio = 0.0;
    config.min_messages = 2;
    config.keep_ratio = 0.2;
    config.floor_tokens = 100;
    let conn = database::db::get_connection().unwrap();
    crate::model_context::weighted_usage::record_with_connection(
        &conn,
        sid,
        "threshold",
        0,
        5_000_000_000,
    )
    .unwrap();
}

async fn seed_sm_baseline(processor: &UnifiedMessageProcessor) {
    let mut state = processor.sm_state.lock().await;
    state.content = Some("old memory".into());
    state.last_summarized_seq = Some(3);
    state.tokens_at_last_extraction = Some(20_000);
    state.initialized = true;
    state.extraction_generation = 7;
    drop(state);
    processor
        .session
        .last_context_tokens
        .store(10_000, Ordering::SeqCst);
}

fn boundary_rows(sid: &str) -> i64 {
    database::db::get_connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND compact_from_sequence IS NOT NULL",
            [sid],
            |r| r.get(0),
        )
        .unwrap()
}

/// R1/R4: a provider summary that completes only after >60s, cancelled while
/// in flight, publishes nothing and resets nothing; the next pre-turn run
/// re-triggers from the same baseline and commits exactly once.
/// Real 61s wall-clock wait: the pre-turn path uses `block_in_place`, which
/// rules out a paused current-thread runtime.
#[tokio::test(flavor = "multi_thread")]
async fn compaction_pre_turn_cancel_after_61s_summary_preserves_baseline_then_recovers() {
    use crate::core::model_context::phase_observation::capture::Capture;
    use tracing::instrument::WithSubscriber;
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pre-turn-cancel-61s";
    let provider = Arc::new(GatedProvider {
        calls: AtomicUsize::new(0),
        gate: std::sync::atomic::AtomicBool::new(true),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let mut processor = fixture_with(sid, provider.clone());
    arm_weighted_pre_turn(&mut processor, sid);
    seed_sm_baseline(&processor).await;
    let before = crate::model_context::weighted_usage::snapshot(sid).unwrap();
    let original = unified_persistence::load_llm_history(sid).unwrap();
    let mut messages = original.clone();
    let capture = Capture::default();
    let started = std::time::Instant::now();
    let (outcome, ()) = async {
        tokio::join!(processor.run_pre_turn_compaction(sid, &mut messages), async {
            provider.entered.notified().await;
            tokio::time::sleep(std::time::Duration::from_secs(61)).await;
            processor.session.cancel_flag.store(true, Ordering::SeqCst);
            provider.release.notify_one();
        })
    }
    .with_subscriber(capture.clone())
    .await;
    assert!(started.elapsed() >= std::time::Duration::from_secs(61));
    assert!(matches!(outcome, CompactionPhaseOutcome::Continue));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1, "no retry/fallback after a successful summary");
    assert_eq!(messages, original, "uncommitted candidate history must not be published");
    assert_eq!(boundary_rows(sid), 0);
    let after = crate::model_context::weighted_usage::snapshot(sid).unwrap();
    assert_eq!((after.milli, after.generation), (before.milli, before.generation));
    assert_eq!(processor.session.last_context_tokens.load(Ordering::SeqCst), 10_000);
    {
        let state = processor.sm_state.lock().await;
        assert_eq!(state.last_summarized_seq, Some(3));
        assert_eq!(state.tokens_at_last_extraction, Some(20_000));
        assert_eq!(state.extraction_generation, 7);
    }
    {
        let state = processor.session.compaction.lock().await;
        assert!(state.summary.is_none(), "phantom summary survived cancel");
        assert_eq!(state.recompaction_info.compaction_count, 0);
        assert_eq!(state.consecutive_failures, 0);
    }
    assert_eq!(unified_persistence::load_session_memory_state(sid).unwrap().last_seq, Some(3));
    // Phase evidence: the adapter call itself spanned the 61s and finished ok.
    let request = capture.assert_phase("fork_adapter_call", "ok");
    {
        let rows = capture.0.lock().unwrap();
        let call = rows.iter().find(|r| r.get("phase") == Some(&"\"fork_adapter_call\"".into())
            && r.get("status") == Some(&"\"ok\"".into())).unwrap();
        assert!(call["elapsed_us"].parse::<u64>().unwrap() >= 61_000_000, "{call:?}");
        let total = rows.iter().find(|r| r.get("stage") == Some(&"\"compactor_total\"".into())).unwrap();
        assert_eq!(total["request"], request.to_string());
        assert!(total["elapsed_ms"].parse::<u64>().unwrap() >= 61_000, "{total:?}");
    }
    capture.assert_secret_absent();

    // Next turn: cancel cleared at send, provider answers promptly.
    processor.session.cancel_flag.store(false, Ordering::SeqCst);
    provider.gate.store(false, Ordering::SeqCst);
    let mut messages = original.clone();
    assert!(matches!(
        processor.run_pre_turn_compaction(sid, &mut messages).await,
        CompactionPhaseOutcome::Continue
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_ne!(messages, original);
    assert_eq!(boundary_rows(sid), 1);
    let committed = crate::model_context::weighted_usage::snapshot(sid).unwrap();
    assert_eq!((committed.milli, committed.generation), (0, before.generation + 1));
    assert_eq!(processor.session.last_context_tokens.load(Ordering::SeqCst), 0);
    {
        let state = processor.sm_state.lock().await;
        assert_eq!(state.last_summarized_seq, None);
        assert_eq!(state.extraction_generation, 8);
        assert_eq!(state.content.as_deref(), Some("old memory"));
    }
    assert_eq!(processor.session.compaction.lock().await.recompaction_info.compaction_count, 1);
    let durable = unified_persistence::load_session_memory_state(sid).unwrap();
    assert_eq!((durable.content.as_deref(), durable.last_seq), (Some("old memory"), None));
}

/// R1: a failed spend-triggered pre-turn compaction keeps the accounting and
/// SM baseline; only the circuit-breaker counter advances.
#[tokio::test(flavor = "multi_thread")]
async fn compaction_pre_turn_failure_preserves_weighted_and_observed_baseline() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pre-turn-failure-baseline";
    let (mut processor, provider) = fixture(sid, true);
    arm_weighted_pre_turn(&mut processor, sid);
    seed_sm_baseline(&processor).await;
    let before = crate::model_context::weighted_usage::snapshot(sid).unwrap();
    let original = unified_persistence::load_llm_history(sid).unwrap();
    let mut messages = original.clone();
    assert!(matches!(
        processor.run_pre_turn_compaction(sid, &mut messages).await,
        CompactionPhaseOutcome::Continue
    ));
    assert!(provider.calls.load(Ordering::SeqCst) >= 2, "fork then side-query fallback");
    assert_eq!(messages, original);
    assert_eq!(boundary_rows(sid), 0);
    let after = crate::model_context::weighted_usage::snapshot(sid).unwrap();
    assert_eq!((after.milli, after.generation), (before.milli, before.generation));
    assert_eq!(processor.session.last_context_tokens.load(Ordering::SeqCst), 10_000);
    let state = processor.sm_state.lock().await;
    assert_eq!((state.last_summarized_seq, state.extraction_generation), (Some(3), 7));
    drop(state);
    let compaction = processor.session.compaction.lock().await;
    assert!(compaction.summary.is_none());
    assert_eq!(compaction.consecutive_failures, 1);
}

/// Spend-only arming: large window + ratio 1.0 so only the weighted trigger can fire.
fn arm_spend_only(processor: &mut UnifiedMessageProcessor, sid: &str, agent_threshold: u64) {
    processor.sm_config.enabled = false;
    let resolved = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved;
    resolved.context_window = 100_000;
    let config = &mut resolved.compaction;
    config.enabled = true;
    config.weighted_token_threshold = agent_threshold;
    config.trigger_ratio = 1.0;
    config.min_messages = 2;
    config.keep_ratio = 0.2;
    config.floor_tokens = 100;
    let conn = database::db::get_connection().unwrap();
    // 5_000_000 equivalent tokens of recorded spend.
    crate::model_context::weighted_usage::record_with_connection(&conn, sid, "spend", 0, 5_000_000_000)
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_session_threshold_override_above_spend_blocks_agent_default_trigger() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pref-override-above";
    let (mut processor, provider) = fixture(sid, false);
    arm_spend_only(&mut processor, sid, 1); // agent default would fire
    crate::session::compaction_prefs::set_threshold(sid, Some(6_000_000)).unwrap();
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    assert!(matches!(
        processor.run_pre_turn_compaction(sid, &mut messages).await,
        CompactionPhaseOutcome::Continue
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(boundary_rows(sid), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_session_threshold_zero_disables_only_spend_trigger() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pref-override-zero";
    let (mut processor, provider) = fixture(sid, false);
    arm_spend_only(&mut processor, sid, 1);
    crate::session::compaction_prefs::set_threshold(sid, Some(0)).unwrap();
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    assert!(matches!(
        processor.run_pre_turn_compaction(sid, &mut messages).await,
        CompactionPhaseOutcome::Continue
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    // Context-capacity safety is independent of the spend override.
    let sid = "pref-override-zero-context";
    let (mut processor, provider) = fixture(sid, false);
    arm_weighted_pre_turn(&mut processor, sid); // window 1000, ratio 0.0
    crate::session::compaction_prefs::set_threshold(sid, Some(0)).unwrap();
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    let _ = processor.run_pre_turn_compaction(sid, &mut messages).await;
    assert!(
        provider.calls.load(Ordering::SeqCst) >= 1,
        "context trigger must still fire with spend disabled"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn compaction_session_threshold_override_below_spend_triggers_when_agent_disabled() {
    let _sandbox = test_helpers::test_env::sandbox();
    let sid = "pref-override-below";
    let (mut processor, provider) = fixture(sid, false);
    arm_spend_only(&mut processor, sid, 0); // agent spend trigger disabled
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    let _ = processor.run_pre_turn_compaction(sid, &mut messages).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0, "no override: stays off");

    crate::session::compaction_prefs::set_threshold(sid, Some(4_000_000)).unwrap();
    let mut messages = unified_persistence::load_llm_history(sid).unwrap();
    let _ = processor.run_pre_turn_compaction(sid, &mut messages).await;
    assert!(
        provider.calls.load(Ordering::SeqCst) >= 1,
        "override below recorded spend must trigger at the next turn"
    );
}

// Real processor entry and session mutex; in-process provider fixture, no network/model.
#[tokio::test]
async fn compaction_processor_queued_lock_failure_recovery_cancel_capture() {
    use crate::core::model_context::phase_observation::capture::Capture;
    use tracing::instrument::WithSubscriber;
    let _sandbox = test_helpers::test_env::sandbox();
    for fail in [false, true] {
        let sid = if fail { "processor-capture-failed" } else { "processor-capture-ok" };
        let (mut processor, provider) = fixture(sid, fail);
        let config = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved.compaction;
        config.min_messages = 2;
        config.trigger_ratio = 0.0;
        config.keep_ratio = 0.2;
        config.floor_tokens = 100;
        let messages = unified_persistence::load_llm_history(sid).unwrap();
        let capture = Capture::default();
        async {
            let held = processor.session.compaction.lock().await;
            let mut call = Box::pin(processor.reactive_llm_compact_with_rescue(sid, &messages, 1000));
            // Poll the real processor to Pending before starting the dwell timer.
            // select! can poll an already-ready timer first after executor stalls,
            // so its sleep arm alone does not prove the mutex waiter was queued.
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(call.as_mut(), cx).is_pending(),
                    "processor bypassed held mutex");
                std::task::Poll::Ready(())
            }).await;
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            drop(held);
            let (_, outcome) = call.await;
            assert_eq!(matches!(outcome, CompactionOutcome::Failed { .. }), fail);
            let request = capture.assert_phase("state_lock_wait", "ok");
            assert_eq!(capture.assert_phase("side_query_adapter_call", if fail { "failed" } else { "ok" }), request);
            let rows = capture.0.lock().unwrap();
            let lock_row = rows.iter().find(|r| r.get("request") == Some(&request.to_string()) && r.get("phase") == Some(&"\"state_lock_wait\"".into()) && r.get("status") == Some(&"\"ok\"".into())).unwrap();
            assert!(lock_row["elapsed_us"].parse::<u64>().unwrap() >= 10000, "lock capture: {lock_row:?}");
            eprintln!("verified real processor lock capture: {lock_row:?}");
            drop(rows);
            assert!(provider.calls.load(Ordering::SeqCst) > 0);
            assert!(processor.session.compaction.try_lock().is_ok());
        }.with_subscriber(capture.clone()).await;
        capture.assert_secret_absent();
        let cancel = Capture::default();
        async {
            let held = processor.session.compaction.lock().await;
            let before = provider.calls.load(Ordering::SeqCst);
            let mut call = Box::pin(processor.reactive_llm_compact_with_rescue(sid, &messages, 1000));
            // Poll the real processor to Pending before starting the dwell timer.
            // select! can poll an already-ready timer first after executor stalls,
            // so its sleep arm alone does not prove the mutex waiter was queued.
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(call.as_mut(), cx).is_pending(),
                    "processor bypassed held mutex");
                std::task::Poll::Ready(())
            }).await;
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            drop(call);
            cancel.assert_phase("state_lock_wait", "interrupted");
            assert_eq!(provider.calls.load(Ordering::SeqCst), before);
            drop(held);
            assert!(processor.session.compaction.try_lock().is_ok());
        }.with_subscriber(cancel.clone()).await;
    }
}

// Uses the default Wry runtime; the context supplies only empty test assets.
#[cfg(feature = "real-wry-tests")]
#[test]
fn compaction_real_wry_display_entry_probe() {
    use tauri::Manager;
    let _sandbox = test_helpers::test_env::sandbox();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(crate::state::AgentAppState::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("real default Wry Builder");
    let _managed = app.state::<crate::state::AgentAppState>();
    eprintln!("REAL_WRY_MANAGED_ENTRY_OK");
}

#[cfg(feature = "real-wry-tests")]
#[test]
fn compaction_real_wry_preturn_fork_publication() {
    use tauri::Manager;
    use tracing::instrument::WithSubscriber;
    use crate::core::model_context::phase_observation::capture::Capture;
    let _sandbox = test_helpers::test_env::sandbox();
    let app = tauri::Builder::default().any_thread()
        .manage(crate::state::AgentAppState::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let sid = "osagent-real-wry-preturn";
        let (mut processor, provider) = fixture(sid, false);
        let conn = database::db::get_connection().unwrap();
        crate::persistence::test_schema::ensure_session_events_schema(&conn);
        database::db::init_shell_replay_tables(&conn).unwrap();
        let state = app.state::<crate::state::AgentAppState>();
        state.gateway_bindings.load_from_db().await.unwrap();
        let key = crate::integrations::gateway::SessionKey("telegram:wry-preturn".into());
        state.gateway_bindings.set(key.clone(), sid.into()).await.unwrap();
        processor.app_handle = Some(app.handle().clone());
        processor.sm_config.enabled = false;
        Arc::get_mut(&mut processor.runtime).unwrap().resolved.context_window = 1000;
        let config = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved.compaction;
        config.enabled = true;
        config.weighted_token_threshold = 1;
                config.min_messages = 2;
        config.trigger_ratio = 0.0;
        config.keep_ratio = 0.2;
        config.floor_tokens = 100;
        crate::model_context::weighted_usage::record_with_connection(&conn, sid, "threshold", 0, 5_000_000_000).unwrap();
        let checkpoint = crate::model_context::weighted_usage::snapshot_with_connection(&conn, sid).unwrap();
        let mut messages = unified_persistence::load_llm_history(sid).unwrap();
        let capture = Capture::default();
        let outcome = processor.run_pre_turn_compaction(sid, &mut messages).with_subscriber(capture.clone()).await;
        let CompactionPhaseOutcome::ForkRedirect(result) = outcome else { panic!("real preturn did not redirect") };
        let new_sid = result.fork_redirect.unwrap();
        assert!(provider.calls.load(Ordering::SeqCst) > 0);
        let request = capture.assert_phase("state_lock_wait", "ok");
        assert_eq!(capture.assert_phase("fork_seed", "ok"), request);
        assert_eq!(capture.assert_phase("fork_publication", "ok"), request);
        let restarted = crate::integrations::gateway::binding::BindingStore::new();
        restarted.load_from_db().await.unwrap();
        assert_eq!(restarted.get(&key).await.unwrap().target_session_id, new_sid);
        assert_eq!(unified_persistence::get_session(sid).unwrap().unwrap().status, "archived");
        assert!(!unified_persistence::load_llm_history(&new_sid).unwrap().is_empty());
        let after = crate::model_context::weighted_usage::snapshot_with_connection(&conn, sid).unwrap();
        assert_eq!(after.milli, 0);
        assert_eq!(after.generation, checkpoint.generation + 1);
        assert_eq!(crate::model_context::weighted_usage::snapshot_with_connection(&conn, &new_sid).unwrap().milli, 0);
        eprintln!("REAL_WRY_PRETURN_FORK_OK request={request} new={new_sid}");
    });
}

#[cfg(feature = "real-wry-tests")]
#[test]
fn compaction_real_wry_reactive_then_checkpoint_fork() {
    use tauri::Manager;
    use tracing::instrument::WithSubscriber;
    use crate::core::model_context::phase_observation::capture::Capture;
    let _sandbox = test_helpers::test_env::sandbox();
    let app = tauri::Builder::default().any_thread()
        .manage(crate::state::AgentAppState::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let sid = "osagent-real-wry-reactive-checkpoint";
        let (mut processor, provider) = fixture(sid, false);
        let conn = database::db::get_connection().unwrap();
        crate::persistence::test_schema::ensure_session_events_schema(&conn);
        database::db::init_shell_replay_tables(&conn).unwrap();
        let state = app.state::<crate::state::AgentAppState>();
        state.gateway_bindings.load_from_db().await.unwrap();
        let key = crate::integrations::gateway::SessionKey("telegram:wry-reactive-checkpoint".into());
        state.gateway_bindings.set(key.clone(), sid.into()).await.unwrap();
        processor.app_handle = Some(app.handle().clone());
        processor.sm_config.enabled = false;
        Arc::get_mut(&mut processor.runtime).unwrap().resolved.context_window = 1000;
        let config = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved.compaction;
        config.enabled = true;
        config.weighted_token_threshold = 1;
                config.min_messages = 2;
        config.trigger_ratio = 0.0;
        config.keep_ratio = 0.2;
        config.floor_tokens = 100;
        crate::model_context::weighted_usage::record_with_connection(&conn, sid, "threshold", 0, 5_000_000_000).unwrap();
        let checkpoint = crate::model_context::weighted_usage::snapshot_with_connection(&conn, sid).unwrap();
        let mut messages = unified_persistence::load_llm_history(sid).unwrap();
        // Reactive rejection cannot fork: the rejected prefix must first shrink.
        let reactive = processor.run_reactive_compaction(sid, &mut messages, Some("context length exceeded")).await;
        assert!(matches!(reactive, CompactionOutcome::Compacted { .. }));
        assert_eq!(state.gateway_bindings.get(&key).await.unwrap().target_session_id, sid);
        assert_eq!(unified_persistence::get_session(sid).unwrap().unwrap().status, "running");
        let pending = crate::model_context::weighted_usage::snapshot_with_connection(&conn, sid).unwrap();
        assert_eq!(pending.milli, checkpoint.milli);
        assert_eq!(pending.generation, checkpoint.generation);
        // Make the next turn eligible; only its durable fork may consume spend.
        for _ in 0..4 {
            messages.push(serde_json::json!({"role":"user", "content":"next turn ".repeat(100)}));
            messages.push(serde_json::json!({"role":"assistant", "content":"done"}));
        }
        let capture = Capture::default();
        let outcome = processor.run_pre_turn_compaction(sid, &mut messages).with_subscriber(capture.clone()).await;
        let CompactionPhaseOutcome::ForkRedirect(result) = outcome else { panic!("real preturn did not redirect") };
        let new_sid = result.fork_redirect.unwrap();
        assert!(provider.calls.load(Ordering::SeqCst) > 0);
        let request = capture.assert_phase("state_lock_wait", "ok");
        assert_eq!(capture.assert_phase("fork_seed", "ok"), request);
        assert_eq!(capture.assert_phase("fork_publication", "ok"), request);
        let restarted = crate::integrations::gateway::binding::BindingStore::new();
        restarted.load_from_db().await.unwrap();
        assert_eq!(restarted.get(&key).await.unwrap().target_session_id, new_sid);
        assert_eq!(unified_persistence::get_session(sid).unwrap().unwrap().status, "archived");
        assert!(!unified_persistence::load_llm_history(&new_sid).unwrap().is_empty());
        let after = crate::model_context::weighted_usage::snapshot_with_connection(&conn, sid).unwrap();
        assert_eq!(after.milli, 0);
        assert_eq!(after.generation, checkpoint.generation + 1);
        assert_eq!(crate::model_context::weighted_usage::snapshot_with_connection(&conn, &new_sid).unwrap().milli, 0);
        eprintln!("REAL_WRY_REACTIVE_CHECKPOINT_FORK_OK request={request} new={new_sid}");
    });
}

mod idle_compaction_e2e {
    use super::*;
    use crate::session::compaction_prefs::{self as prefs, idle_monitor};
    use crate::state::commands::session::{
        run_scheduled_compaction, CompactionTrigger, ManualCompactStatus,
    };
    use crate::state::AgentAppState;

    async fn live_state(sid: &str) -> (AgentAppState, Arc<FixtureProvider>, Arc<AgentSession>) {
        let (mut processor, provider) = fixture(sid, false);
        {
            // Small fixture transcript: shrink the keep window so a segment is compactable.
            let config = &mut Arc::get_mut(&mut processor.runtime).unwrap().resolved.compaction;
            config.keep_ratio = 0.2;
            config.floor_tokens = 100;
            config.min_messages = 2;
        }
        let state = AgentAppState::new();
        let def = AgentDefinition {
            selected_model_id: Some("test/model".into()),
            ..Default::default()
        };
        let session = state.register_session(AgentSession::new(sid.into(), def)).await;
        session.set_runtime(processor.runtime.clone()).await.unwrap();
        (state, provider, session)
    }

    fn opt_in(sid: &str) -> i64 {
        let stored = prefs::set_idle(sid, true, 59).unwrap();
        idle_monitor::configure(&stored);
        idle_monitor::note_turn_activity(sid);
        idle_monitor::live_view(sid).unwrap().0.expect("epoch stamped")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stale_epoch_makes_no_model_call_and_changes_nothing() {
        let _sandbox = test_helpers::test_env::sandbox();
        let sid = "idle-e2e-stale";
        let (state, provider, _session) = live_state(sid).await;
        let epoch = opt_in(sid);
        let before = unified_persistence::load_llm_history(sid).unwrap().len();

        let result = run_scheduled_compaction(
            &state,
            sid,
            None,
            CompactionTrigger::Idle { epoch_ms: epoch - 1 },
        )
        .await
        .unwrap();

        assert_eq!(result.status, ManualCompactStatus::Skipped);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(unified_persistence::load_llm_history(sid).unwrap().len(), before);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn switch_turned_off_between_decision_and_job_skips() {
        let _sandbox = test_helpers::test_env::sandbox();
        let sid = "idle-e2e-off";
        let (state, provider, _session) = live_state(sid).await;
        let epoch = opt_in(sid);
        idle_monitor::configure(&prefs::set_idle(sid, false, 59).unwrap());

        let result = run_scheduled_compaction(&state, sid, None, CompactionTrigger::Idle { epoch_ms: epoch })
            .await
            .unwrap();

        assert_eq!(result.status, ManualCompactStatus::Skipped);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn current_epoch_compacts_once_without_rearming_activity() {
        let _sandbox = test_helpers::test_env::sandbox();
        let sid = "idle-e2e-fire";
        let (state, provider, _session) = live_state(sid).await;
        let epoch = opt_in(sid);
        let before = unified_persistence::load_llm_history(sid).unwrap().len();

        let first = run_scheduled_compaction(&state, sid, None, CompactionTrigger::Idle { epoch_ms: epoch })
            .await
            .unwrap();
        eprintln!(
            "IDLE_E2E first status={} calls={} before={before} msgs_after={:?}",
            first.status.as_str(),
            provider.calls.load(Ordering::SeqCst),
            first.messages_after
        );
        assert_eq!(first.status, ManualCompactStatus::Compacted, "{:?}", first.message);
        assert!(first.boundary.is_some());
        assert!(provider.calls.load(Ordering::SeqCst) <= 1);
        assert_eq!(
            idle_monitor::live_view(sid).unwrap().0,
            Some(epoch),
            "the compaction job itself must not count as activity"
        );

        let calls_after_first = provider.calls.load(Ordering::SeqCst);
        let again = run_scheduled_compaction(&state, sid, None, CompactionTrigger::Idle { epoch_ms: epoch })
            .await
            .unwrap();
        // After 8 -> 2 rows the transcript is below the manual minimum; either way no model call.
        assert!(matches!(again.status, ManualCompactStatus::TooShort | ManualCompactStatus::AlreadyCompact), "{:?}", again.status);
        assert_eq!(provider.calls.load(Ordering::SeqCst), calls_after_first);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn idle_never_initializes_a_runtime_for_an_unloaded_session() {
        let _sandbox = test_helpers::test_env::sandbox();
        let conn = database::db::get_connection().unwrap();
        crate::persistence::test_schema::ensure_agent_sessions_schema(&conn);
        let state = AgentAppState::new();
        let sid = "idle-e2e-unloaded";
        conn.execute("INSERT INTO agent_sessions (session_id,name,session_type,status,created_at,updated_at) VALUES (?1,'x','agent','idle',datetime('now'),datetime('now'))", [sid]).unwrap();

        let result = run_scheduled_compaction(&state, sid, None, CompactionTrigger::Idle { epoch_ms: 1 })
            .await
            .unwrap();

        assert_eq!(result.status, ManualCompactStatus::NoRuntime);
        assert!(state.get_session(sid).await.is_none(), "idle must not lazy-init");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn channel_attached_session_is_refused_for_idle() {
        let _sandbox = test_helpers::test_env::sandbox();
        let sid = "idle-e2e-channel";
        let (state, provider, _session) = live_state(sid).await;
        let epoch = opt_in(sid);
        state
            .gateway_bindings
            .set(
                crate::integrations::gateway::binding::SessionKey("chan:1".into()),
                sid.into(),
            )
            .await
            .unwrap();

        let result = run_scheduled_compaction(&state, sid, None, CompactionTrigger::Idle { epoch_ms: epoch })
            .await
            .unwrap();

        assert_eq!(result.status, ManualCompactStatus::ChannelAttached);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
}
