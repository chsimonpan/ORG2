mod bounded;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_fs::FsExt;
use std::sync::Mutex;
// Serial native I/O bounds simultaneous body materialization independently
// of frontend save queues. Never hold a session/model lock here.
static EDITOR_IO: Mutex<()> = Mutex::new(());

// A persisted user grant does not become an unrestricted FS scope. Re-check
// read/write mode on every native boundary, so revoke is immediately effective.
fn editor_authorized<R: Runtime>(app: &AppHandle<R>, path: &std::path::Path, access: agent_core::foundation::security::global_paths::policy::Access) -> Result<bool, String> {
    use agent_core::foundation::security::global_paths;
    global_paths::check_store()?;
    let explicit = global_paths::allowed(path, access)?;
    Ok(explicit || app.try_fs_scope().ok_or("editor filesystem scope unavailable")?.is_allowed(path))
}

#[tauri::command]
pub async fn editor_stat_bounded<R: Runtime>(app: AppHandle<R>, path: String) -> Result<serde_json::Value, String> {
    let path = permitted(&app, path, agent_core::foundation::security::global_paths::policy::Access::Read)?;
    tauri::async_runtime::spawn_blocking(move || {
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("editor metadata: {e}"))?;
        if !meta.is_file() { return Err("editor path is not a regular file".into()); }
        Ok(serde_json::json!({"isFile":true,"size":meta.len()}))
    }).await.map_err(|e| format!("metadata task: {e}"))?
}

fn permitted<R: Runtime>(app: &AppHandle<R>, path: String, access: agent_core::foundation::security::global_paths::policy::Access) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() { return Err("editor path must be absolute".into()); }
    if !editor_authorized(app, &path, access)? { return Err("editor filesystem scope denied".into()); }
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) { return Err("symlinks not editable".into()); }
    let resolved = match path.canonicalize() {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or("editor path needs parent")?
                .canonicalize().map_err(|e| format!("editor parent: {e}"))?;
            parent.join(path.file_name().ok_or("editor path needs filename")?)
        }
        Err(error) => return Err(format!("editor path: {error}")),
    };
    if !editor_authorized(app, &resolved, access)? { return Err("editor resolved filesystem scope denied".into()); }
    // Operate on the resolved path; final-component symlinks are rejected in core.
    Ok(resolved)
}

#[tauri::command]
pub async fn editor_read_bounded<R: Runtime>(app: AppHandle<R>, path: String) -> Result<String, String> {
    let path = permitted(&app, path, agent_core::foundation::security::global_paths::policy::Access::Read)?;
    tauri::async_runtime::spawn_blocking(move || { let _guard = EDITOR_IO.lock().map_err(|_| "editor I/O lock failed")?; bounded::read_bounded(&path) }).await.map_err(|e| format!("read task: {e}"))?
}

#[tauri::command]
pub async fn editor_write_bounded<R: Runtime>(app: AppHandle<R>, path: String, content: String, create: Option<bool>, expected_content: Option<String>) -> Result<(), String> {
    if content.len() as u64 > bounded::LARGE_FILE_LIMIT_BYTES || expected_content.as_ref().is_some_and(|s| s.len() as u64 > bounded::LARGE_FILE_LIMIT_BYTES) { return Err("too_large: editor payload".into()); }
    let path = permitted(&app, path, agent_core::foundation::security::global_paths::policy::Access::Write)?;
    tauri::async_runtime::spawn_blocking(move || { let _guard = EDITOR_IO.lock().map_err(|_| "editor I/O lock failed")?; bounded::write_bounded(&path, &content, create.unwrap_or(false), expected_content.as_deref()) }).await.map_err(|e| format!("save task: {e}"))?
}

#[cfg(test)]
mod rpc_tests;
