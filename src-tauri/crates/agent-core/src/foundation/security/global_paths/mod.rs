//! Global path grant application service; no model/agent can mutate grants.
pub mod policy;
use policy::{Access, Grant};
use std::path::{Path, PathBuf};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathExemption {
    pub path: String,
    pub mode: String,
    pub recursive: bool,
}
fn store() -> PathBuf {
    app_paths::orgii_root().join("global_path_exemptions.v1")
}
fn dto(grants: Vec<Grant>) -> Vec<PathExemption> {
    grants
        .into_iter()
        .map(|g| PathExemption {
            path: g.path.to_string_lossy().into(),
            mode: if g.write { "read_write" } else { "read" }.into(),
            recursive: g.recursive,
        })
        .collect()
}
pub fn allowed(path: &Path, access: Access) -> Result<bool, String> {
    policy::matches(&policy::load(&store())?, path, access)
}
pub fn check_store() -> Result<(), String> {
    policy::load(&store()).map(|_| ())
}
#[tauri::command]
pub async fn global_path_exemptions_list() -> Result<Vec<PathExemption>, String> {
    tauri::async_runtime::spawn_blocking(|| policy::load(&store()).map(dto))
        .await
        .map_err(|_| "grant list task failed")?
}
#[tauri::command]
pub async fn global_path_exemptions_add(
    path: String,
    mode: String,
    recursive: bool,
) -> Result<Vec<PathExemption>, String> {
    if !Path::new(&path).is_absolute() {
        return Err("grant path must be absolute".into());
    }
    let access = match mode.as_str() {
        "read" => Access::Read,
        "read_write" => Access::Write,
        _ => return Err("invalid grant mode".into()),
    };
    tauri::async_runtime::spawn_blocking(move || {
        policy::add(&store(), Path::new(&path), access, recursive).map(dto)
    })
    .await
    .map_err(|_| "grant add task failed")?
}
#[tauri::command]
pub async fn global_path_exemptions_remove(path: String) -> Result<Vec<PathExemption>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        policy::remove(&store(), Path::new(&path)).map(dto)
    })
    .await
    .map_err(|_| "grant remove task failed")?
}

/// Guard runs before routers, caches and path-recovery. It cannot grant a shell
/// command capability, remove an autonomy denial, or mutate session state.
pub fn authorize(
    raw: &str,
    primary: Option<&Path>,
    extras: &[PathBuf],
    security: Option<&super::SecurityPolicy>,
    access: Access,
) -> Result<(), String> {
    if raw.contains('\0') {
        return Err("NUL path denied".into());
    }
    let expanded = if let Some(suffix) = raw.strip_prefix("~/") {
        app_paths::home_dir().join(suffix)
    } else {
        PathBuf::from(raw)
    };
    let candidate = if expanded.is_absolute() {
        expanded
    } else {
        primary
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir().map_err(|_| "working directory unavailable")?)
            .join(expanded)
    };
    let resolved = policy::canonical_candidate(&candidate)?;
    let store_path = store();
    let protected_parent =
        policy::canonical_candidate(store_path.parent().ok_or("invalid policy parent")?)?;
    if resolved.parent() == Some(protected_parent.as_path())
        && resolved
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with("global_path_exemptions."))
    {
        return Err("grant policy storage is user-management-only".into());
    }
    if let Some(policy) = security {
        policy.validate_path_syntax(candidate.to_str().ok_or("invalid path encoding")?)?;
    }
    if security.is_some_and(|policy| policy.workspace_only) && primary.is_none() {
        return Err("restricted policy requires session workspace".into());
    }
    let mut local = extras.to_vec();
    if let Some(p) = primary {
        local.push(p.to_path_buf());
    }
    policy::authorize(&store(), &candidate, access, &local, &[], primary.is_some())
}

pub fn authorize_tree(
    candidate: &Path,
    primary: Option<&Path>,
    extras: &[PathBuf],
    security: Option<&super::SecurityPolicy>,
) -> Result<(), String> {
    authorize(
        candidate.to_str().ok_or("invalid scan path")?,
        primary,
        extras,
        security,
        Access::Read,
    )?;
    let mut local = extras.to_vec();
    if let Some(p) = primary {
        local.push(p.to_path_buf());
    }
    let denied = security
        .map(|p| {
            p.forbidden_paths
                .iter()
                .map(|p| {
                    if let Some(s) = p.strip_prefix("~/") {
                        app_paths::home_dir().join(s)
                    } else {
                        PathBuf::from(p)
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    policy::authorize_tree(&store(), candidate, &local, &denied, primary.is_some())
}
