//! Tauri adapters for compressed Task forks. Deliberately thin: every invariant
//! (publish-after-compression, exactly-once append, owner isolation) lives in
//! `agent_core::core::session::task_fork`. No command here ever calls a model
//! other than the session's own resolved runtime.
use std::sync::Arc;
use std::time::Duration;

use agent_core::core::session::task_fork as tf;
use tauri::Manager;

type State = agent_core::state::AgentAppState;

fn check_owner(owner: &str) -> Result<(), String> {
    if owner.is_empty() || owner.starts_with("cli:") {
        return Err("journey_owner_mismatch".into());
    }
    Ok(())
}

/// Claim a session for a short exclusive mutation. The turn pipeline marks a running turn with
/// `active_turn` (not with `processing_lock`, which only the Journey/import commands contend on),
/// so both are consulted: a running turn or a competing command makes the session busy.
async fn idle_claim(
    session: &agent_core::state::AgentSession,
) -> Result<tokio::sync::OwnedMutexGuard<()>, ()> {
    let guard = session
        .processing_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| ())?;
    match session.active_turn.try_lock() {
        Ok(turn) if turn.is_none() => Ok(guard),
        _ => Err(()),
    }
}

/// Summarizer bound to `session_id`'s live runtime, if it has one.
async fn runtime_summarizer(
    state: &State,
    session_id: &str,
) -> Result<Option<Arc<dyn tf::TaskForkSummarizer>>, String> {
    let Some(session) = state.get_session(session_id).await else {
        return Ok(None);
    };
    let Some(runtime) = session.get_runtime().await else {
        return Ok(None);
    };
    let summarizer = tf::SideQuerySummarizer::new(
        runtime.provider.clone(),
        runtime.model.clone(),
        runtime.account_id.clone(),
    )?;
    Ok(Some(Arc::new(summarizer)))
}

#[derive(Clone)]
pub struct TauriTaskForkHost {
    state: State,
}

impl TauriTaskForkHost {
    pub fn new(state: State) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl tf::TaskForkHost for TauriTaskForkHost {
    async fn summarizer_for_finish(
        &self,
        link: &tf::TaskForkLink,
    ) -> Result<Arc<dyn tf::TaskForkSummarizer>, String> {
        let state = &self.state;
        // The model the Task actually ran on: its own runtime first.
        if let Some(child) = &link.child_session_id {
            if let Some(summarizer) = runtime_summarizer(state, child).await? {
                return Ok(summarizer);
            }
        }
        // Otherwise the parent's runtime, but only when it is the very same
        // model/account/protocol the Task was compressed with — never a silent switch.
        if let Some(summarizer) = runtime_summarizer(state, &link.parent_session_id).await? {
            let p = summarizer.provenance();
            if p.model_id == link.model_id
                && p.account_id == link.account_id
                && p.protocol == link.protocol
            {
                return Ok(summarizer);
            }
            return Err(format!(
                "task_fork_runtime_unavailable: 子会话无运行时，且主线运行时（{}）与创建 Task 时的模型（{}）不一致，拒绝切换模型",
                p.model_id, link.model_id
            ));
        }
        Err(
            "task_fork_runtime_unavailable: 子会话与主线都没有活动运行时；打开该会话后会自动继续"
                .into(),
        )
    }

    async fn try_lock_parent(
        &self,
        parent_session_id: &str,
    ) -> Result<tf::ParentLockGuard, String> {
        match self.state.get_session(parent_session_id).await {
            None => Ok(tf::ParentLockGuard::unregistered()),
            Some(session) => idle_claim(&session)
                .await
                .map(tf::ParentLockGuard::held)
                .map_err(|_| "parent_busy".to_string()),
        }
    }

    async fn publish_message_event(
        &self,
        session_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<(), String> {
        let app = self
            .state
            .app_handle
            .clone()
            .ok_or_else(|| "ui_bus_unavailable: 应用句柄尚未注入".to_string())?;
        let (session_id, message_id, content) = (
            session_id.to_string(),
            message_id.to_string(),
            content.to_string(),
        );
        // Same persistence path ordinary user messages use; the event id is derived from the
        // message id, so repeating this call is an idempotent upsert.
        tokio::task::spawn_blocking(move || {
            agent_core::bus::event_pipeline_bridge::persist_user_message_event(
                &app,
                &session_id,
                &message_id,
                &content,
                None,
                None,
                agent_core::bus::event_pipeline_bridge::PersistedUserMessageSource::User,
                "",
            )
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Create a Task: compress the frozen main-line prefix with the session's own
/// runtime, then publish an independent child that contains only that summary.
#[tauri::command]
pub async fn task_fork_create(
    owner_session_id: String,
    request: tf::CreateTaskForkRequest,
    state: tauri::State<'_, State>,
) -> Result<tf::TaskForkView, String> {
    check_owner(&owner_session_id)?;
    if state.get_session(&owner_session_id).await.is_none() {
        return Err("journey_owner_not_registered".into());
    }
    let Some(summarizer) = runtime_summarizer(&state, &owner_session_id).await? else {
        return Err(
            "task_fork_runtime_unavailable: 主线会话还没有活动运行时，请先发送一条消息再创建 Task"
                .into(),
        );
    };
    let view = tf::create_task_fork(&owner_session_id, request, summarizer.as_ref()).await?;
    // A durable child must also be a runnable runtime; replays heal a failed rehydrate.
    if let Some(child) = &view.link.child_session_id {
        if state.get_session(child).await.is_none() {
            agent_core::init::register_session_with_rehydrate(&state, child).await?;
        }
    }
    // Show the compressed seed in the Task's own chat (idempotent; best effort — the seed is
    // already durable and provider-visible either way).
    {
        use tf::TaskForkHost;
        let view_for_seed = view.clone();
        if let Some((child, seed_id, content)) =
            tokio::task::spawn_blocking(move || tf::seed_event_payload(&view_for_seed))
                .await
                .map_err(|e| e.to_string())?
        {
            if let Err(error) = TauriTaskForkHost::new(state.inner().clone())
                .publish_message_event(&child, &seed_id, &content)
                .await
            {
                tracing::warn!(session_id = %child, error = %error, "[task-fork] seed UI event not published");
            }
        }
    }
    Ok(view)
}

/// Finish a Task: freeze it and start the asynchronous, cancellable, durable
/// summary job. Returns immediately; poll `task_fork_list` for progress.
#[tauri::command]
pub async fn task_fork_finish(
    owner_session_id: String,
    request: tf::FinishTaskForkRequest,
    state: tauri::State<'_, State>,
) -> Result<tf::TaskForkView, String> {
    check_owner(&owner_session_id)?;
    let (owner, req) = (owner_session_id.clone(), request.clone());
    let link = tokio::task::spawn_blocking(move || {
        let conn = database::db::get_connection().map_err(|e| e.to_string())?;
        tf::ensure_schema(&conn)?;
        tf::authorize(&conn, &owner, &req.task_fork_id)
    })
    .await
    .map_err(|e| e.to_string())??;
    // The freeze must not race a running Task turn.
    let child_guard = match link.child_session_id.as_deref() {
        Some(child) => match state.get_session(child).await {
            Some(session) => Some(idle_claim(&session).await.map_err(|_| {
                "task_fork_child_busy: Task 仍在处理消息，请等它停止后再结束".to_string()
            })?),
            None => None,
        },
        None => None,
    };
    let (owner, req) = (owner_session_id, request);
    let (view, start_driver) = tokio::task::spawn_blocking(move || tf::begin_finish(&owner, &req))
        .await
        .map_err(|e| e.to_string())??;
    drop(child_guard);
    if start_driver {
        let id = view.link.id.clone();
        let host = Arc::new(TauriTaskForkHost::new(state.inner().clone()));
        tauri::async_runtime::spawn(async move {
            if let Err(error) =
                tf::drive_finish_job(host.as_ref(), &id, tf::DriveOptions::default()).await
            {
                tracing::warn!(task_fork = %id, error = %error, "[task-fork] finish driver ended with an error");
            }
        });
    }
    Ok(view)
}

#[tauri::command]
pub async fn task_fork_cancel_finish(
    owner_session_id: String,
    task_fork_id: String,
) -> Result<tf::TaskForkView, String> {
    check_owner(&owner_session_id)?;
    tokio::task::spawn_blocking(move || tf::cancel_finish(&owner_session_id, &task_fork_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn task_fork_list(owner_session_id: String) -> Result<Vec<tf::TaskForkView>, String> {
    check_owner(&owner_session_id)?;
    tokio::task::spawn_blocking(move || tf::list_for_owner(&owner_session_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Startup + periodic recovery of unfinished summary jobs (crash/restart safe).
pub fn spawn_recovery_worker(app: tauri::AppHandle) {
    let state = app.state::<State>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let host = TauriTaskForkHost::new(state);
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            let options = tf::DriveOptions {
                append_retries: 0,
                retry_delay: Duration::from_millis(0),
            };
            let ran = tf::recover_pending(&host, options).await;
            if ran > 0 {
                tracing::info!(drivers = ran, "[task-fork] recovery sweep");
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    fn invoke(
        w: &tauri::WebviewWindow<MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            w,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: w.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<Value>().unwrap())
    }

    struct FixedSummarizer(&'static str);
    #[async_trait::async_trait]
    impl tf::TaskForkSummarizer for FixedSummarizer {
        fn provenance(&self) -> agent_core::core::session::journey_lifecycle::RuntimeProvenance {
            agent_core::core::session::journey_lifecycle::RuntimeProvenance {
                model_id: "fake/model".into(),
                account_id: "acct".into(),
                protocol: "fake".into(),
            }
        }
        async fn summarize(&self, _r: tf::SummaryRequest<'_>) -> Result<tf::SummaryOutput, String> {
            Ok(tf::SummaryOutput {
                text: self.0.into(),
                usage: vec![],
            })
        }
    }

    /// Registered IPC contract: owner isolation, no-runtime fail-closed create, and a finish that
    /// defers (never fails, never switches model) while no runtime is active, then cancels cleanly.
    #[test]
    fn registered_ipc_owner_isolation_and_no_runtime_behaviour() {
        let _sandbox = test_helpers::test_env::sandbox();
        let owner = "sde-ipc-owner";
        let state = State::new();
        tauri::async_runtime::block_on(state.register_session(
            agent_core::state::AgentSession::new(
                owner.into(),
                agent_core::definitions::sde_agent(),
            ),
        ));
        {
            let c = database::db::get_connection().unwrap();
            agent_core::foundation::persistence::session_snapshots::ensure_tables_with(&c).unwrap();
            c.execute("INSERT INTO agent_sessions(session_id,model,account_id,created_at,updated_at) VALUES(?1,'fake/model','acct','2026-10-12T00:00:00Z','2026-10-12T00:00:00Z')", [owner]).unwrap();
            for (i, (id, role)) in [("m0", "user"), ("m1", "assistant")].iter().enumerate() {
                c.execute("INSERT INTO agent_messages(id,session_id,role,content,sequence,created_at) VALUES(?1,?2,?3,?4,?5,'2026-10-12T00:00:00Z')", rusqlite::params![id, owner, role, format!("内容 {id} ").repeat(80), i as i64]).unwrap();
            }
        }
        let app = mock_builder()
            .manage(state.clone())
            .invoke_handler(tauri::generate_handler![
                task_fork_create,
                task_fork_finish,
                task_fork_cancel_finish,
                task_fork_list
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let w = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        // Foreign / cli owners are rejected before any lookup.
        assert!(invoke(&w, "task_fork_list", json!({"ownerSessionId": "cli:x"})).is_err());
        assert_eq!(
            invoke(
                &w,
                "task_fork_list",
                json!({"ownerSessionId": "sde-stranger"})
            )
            .unwrap(),
            json!([])
        );
        // Create with a registered owner that has no live runtime fails closed with an explicit reason.
        let anchor = {
            let c = database::db::get_connection().unwrap();
            agent_core::core::session::journey_canonical::resolve_with_connection(
                &c,
                owner,
                owner,
                &format!("session:{owner}"),
                "m1",
                1,
            )
            .unwrap()
        };
        let create = json!({"ownerSessionId": owner, "request": {"intentId": "ipc-1", "name": "IPC Task", "anchor": anchor}});
        let e = invoke(&w, "task_fork_create", create).unwrap_err();
        assert!(
            e.as_str()
                .unwrap()
                .starts_with("task_fork_runtime_unavailable"),
            "{e}"
        );
        assert_eq!(
            invoke(&w, "task_fork_list", json!({"ownerSessionId": owner})).unwrap(),
            json!([]),
            "failed create must leave no link"
        );
        // Seed a published Task through the core (the production summarizer needs a live provider).
        let request = tf::CreateTaskForkRequest {
            intent_id: "ipc-2".into(),
            name: "IPC Task".into(),
            anchor: serde_json::from_value(anchor_json(owner)).unwrap(),
        };
        let view = tauri::async_runtime::block_on(tf::create_task_fork(
            owner,
            request,
            &FixedSummarizer("压缩摘要"),
        ))
        .unwrap();
        let id = view.link.id.clone();
        let list = invoke(&w, "task_fork_list", json!({"ownerSessionId": owner})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["state"], "active");
        // Another owner cannot finish or cancel it.
        let finish = |o: &str| json!({"ownerSessionId": o, "request": {"taskForkId": id, "outcome": "completed"}});
        assert_eq!(
            invoke(&w, "task_fork_finish", finish("sde-stranger")).unwrap_err(),
            json!("task_fork_owner_mismatch")
        );
        assert_eq!(
            invoke(
                &w,
                "task_fork_cancel_finish",
                json!({"ownerSessionId": "sde-stranger", "taskForkId": id})
            )
            .unwrap_err(),
            json!("task_fork_owner_mismatch")
        );
        // The owner can: the job is frozen and queued; with no runtime it defers instead of failing.
        let started = invoke(&w, "task_fork_finish", finish(owner)).unwrap();
        assert_eq!(started["state"], "finishing");
        let mut deferred = Value::Null;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let v = invoke(&w, "task_fork_list", json!({"ownerSessionId": owner})).unwrap();
            if v[0]["job"]["deferredReason"].is_string() {
                deferred = v[0].clone();
                break;
            }
        }
        assert_eq!(deferred["job"]["state"], "queued", "{deferred}");
        assert!(deferred["job"]["deferredReason"]
            .as_str()
            .unwrap()
            .starts_with("task_fork_runtime_unavailable"));
        let cancelled = invoke(
            &w,
            "task_fork_cancel_finish",
            json!({"ownerSessionId": owner, "taskForkId": id}),
        )
        .unwrap();
        assert_eq!(
            (
                cancelled["state"].as_str(),
                cancelled["job"]["state"].as_str()
            ),
            (Some("active"), Some("cancelled"))
        );
        let rows: i64 = database::db::get_connection().unwrap().query_row("SELECT COUNT(*) FROM agent_messages WHERE session_id=?1 AND id LIKE 'task-summary:%'", [owner], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
    }

    fn anchor_json(owner: &str) -> Value {
        let c = database::db::get_connection().unwrap();
        serde_json::to_value(
            agent_core::core::session::journey_canonical::resolve_with_connection(
                &c,
                owner,
                owner,
                &format!("session:{owner}"),
                "m1",
                1,
            )
            .unwrap(),
        )
        .unwrap()
    }
}
