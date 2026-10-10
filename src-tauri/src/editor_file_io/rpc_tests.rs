//! Native command dispatcher integration with isolated home and mock webview.
//! This exercises the real Tauri IPC handler and command implementations;
//! it does not pretend to be a rendered desktop end-to-end test.
use super::*;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use serde_json::{json, Value};
use agent_core::foundation::security::global_paths::{self, policy::Access};
use agent_core::foundation::tool_infra::file::resolve_path_with_access;

fn invoke(window: &tauri::WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    invoke_from_url(window, window.url().unwrap(), cmd, body)
}

fn invoke_from_url(window: &tauri::WebviewWindow<MockRuntime>, url: tauri::Url, cmd: &str, body: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(window, tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url,
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    }).map(|body| body.deserialize::<Value>().unwrap())
}

struct StagingEnvGuard(Option<std::ffi::OsString>);
impl StagingEnvGuard {
    fn install(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("ORG2_EDITOR_STAGE_DIR");
        std::env::set_var("ORG2_EDITOR_STAGE_DIR", path);
        Self(previous)
    }
}
impl Drop for StagingEnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.0 { std::env::set_var("ORG2_EDITOR_STAGE_DIR", previous); }
        else { std::env::remove_var("ORG2_EDITOR_STAGE_DIR"); }
    }
}

#[test]
fn real_ipc_scope_bounded_io_and_management_revocation() {
    let sandbox = test_helpers::test_env::sandbox();
    let root = sandbox.path().join("rpc-fixtures");
    std::fs::create_dir(&root).unwrap();
    let stage = root.join("editor-stage");
    std::fs::create_dir(&stage).unwrap();
    let _staging = StagingEnvGuard::install(&stage);
    let file = root.join("small.txt");
    std::fs::write(&file, "before中文").unwrap();
    let app = mock_builder()
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            editor_stat_bounded, editor_read_bounded, editor_write_bounded,
            global_paths::global_path_exemptions_list,
            global_paths::global_path_exemptions_add,
            global_paths::global_path_exemptions_remove,
        ])
        .build(mock_context(noop_assets())).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    assert!(invoke(&window, "editor_read_bounded", json!({"path":file})).is_err());
    invoke(&window, "global_path_exemptions_add", json!({"path":root,"mode":"read","recursive":true})).unwrap();
    assert_eq!(invoke(&window, "editor_stat_bounded", json!({"path":file})).unwrap()["size"], json!("before中文".len()));
    assert_eq!(invoke(&window, "editor_read_bounded", json!({"path":file})).unwrap(), json!("before中文"));
    assert!(invoke(&window,"editor_write_bounded",json!({"path":file,"content":"denied","create":false})).is_err());
    invoke(&window, "global_path_exemptions_remove", json!({"path":root})).unwrap();
    assert!(invoke(&window, "editor_stat_bounded", json!({"path":file})).is_err());
    assert!(invoke(&window, "editor_read_bounded", json!({"path":file})).is_err());
    invoke(&window, "global_path_exemptions_add", json!({"path":root,"mode":"read_write","recursive":true})).unwrap();
    // Resolve the real platform local origin instead of assuming Windows' HTTP
    // workaround URL. An untrusted remote origin must still fail before dispatch,
    // even when the filesystem scope explicitly allows the fixture.
    assert!(invoke_from_url(&window, "https://untrusted.example".parse().unwrap(),
        "editor_read_bounded", json!({"path":file})).is_err());
    assert_eq!(invoke(&window, "editor_read_bounded", json!({"path":file})).unwrap(), json!("before中文"));
    invoke(&window, "editor_write_bounded", json!({"path":file,"content":"after中文","create":false,"expectedContent":"before中文"})).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after中文");
    assert!(invoke(&window, "editor_write_bounded", json!({"path":file,"content":"stale","create":false,"expectedContent":"before中文"})).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "after中文");
    let sparse = root.join("synthetic-564m.txt");
    std::fs::File::create(&sparse).unwrap().set_len(564*1024*1024).unwrap();
    assert!(invoke(&window, "editor_read_bounded", json!({"path":sparse})).unwrap_err().to_string().contains("too_large"));
    assert!(invoke(&window, "editor_write_bounded", json!({"path":sparse,"content":"","create":false})).is_err());
    assert_eq!(std::fs::metadata(&sparse).unwrap().len(),564*1024*1024);
    let new = root.join("new.txt");
    invoke(&window,"editor_write_bounded",json!({"path":new,"content":"new","create":true})).unwrap();
    assert_eq!(invoke(&window,"editor_read_bounded",json!({"path":new})).unwrap(),json!("new"));
    #[cfg(unix)] {
        let link=root.join("linked.txt");std::os::unix::fs::symlink(&file,&link).unwrap();
        assert!(invoke(&window,"editor_read_bounded",json!({"path":link})).is_err());
    }
    invoke(&window,"global_path_exemptions_remove",json!({"path":root})).unwrap();
    assert_eq!(invoke(&window,"global_path_exemptions_list",json!({})).unwrap(),json!([]));
    invoke(&window,"global_path_exemptions_add",json!({"path":root,"mode":"read","recursive":true})).unwrap();
    let workspace=sandbox.path().join("workspace");std::fs::create_dir(&workspace).unwrap();
    assert!(resolve_path_with_access(file.to_str().unwrap(),Some(&workspace),&[],Access::Read).is_ok());
    assert!(resolve_path_with_access(file.to_str().unwrap(),Some(&workspace),&[],Access::Write).is_err());
    assert_eq!(invoke(&window,"global_path_exemptions_list",json!({})).unwrap().as_array().unwrap().len(),1);
    invoke(&window,"global_path_exemptions_add",json!({"path":root,"mode":"read_write","recursive":true})).unwrap();
    assert!(resolve_path_with_access(file.to_str().unwrap(),Some(&workspace),&[],Access::Write).is_ok());
    invoke(&window,"global_path_exemptions_remove",json!({"path":root})).unwrap();
    assert!(resolve_path_with_access(file.to_str().unwrap(),Some(&workspace),&[],Access::Read).is_err());
    assert_eq!(invoke(&window,"global_path_exemptions_list",json!({})).unwrap(),json!([]));
}

/// Real registered IPC with a mock webview, not native GUI acceptance.
#[test]
fn registered_large_file_metadata_read_save_and_revoke_preserve_sentinel() {
    use std::io::{Read, Seek, SeekFrom, Write};
    let sandbox = test_helpers::test_env::sandbox();
    let root = sandbox.path().join("r7-registered-large");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("synthetic-564m.txt");
    let size = 564_u64 * 1024 * 1024;
    let mut file = std::fs::File::create(&path).unwrap();
    file.set_len(size).unwrap();
    file.write_all(b"R7-HEAD").unwrap();
    file.seek(SeekFrom::End(-7)).unwrap();
    file.write_all(b"R7-TAIL").unwrap();
    drop(file);
    let app = mock_builder().plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![editor_stat_bounded, editor_read_bounded,
            editor_write_bounded, global_paths::global_path_exemptions_add,
            global_paths::global_path_exemptions_remove])
        .build(mock_context(noop_assets())).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app,"r7-large",tauri::WebviewUrl::default()).build().unwrap();
    invoke(&window,"global_path_exemptions_add",json!({"path":root,"mode":"read_write","recursive":true})).unwrap();
    assert_eq!(invoke(&window,"editor_stat_bounded",json!({"path":path})).unwrap()["size"],json!(size));
    assert!(invoke(&window,"editor_read_bounded",json!({"path":path})).unwrap_err().to_string().contains("too_large"));
    for content in ["", "replacement"] {
        assert!(invoke(&window,"editor_write_bounded",json!({"path":path,"content":content,"create":false})).unwrap_err().to_string().contains("too_large"));
    }
    invoke(&window,"global_path_exemptions_remove",json!({"path":root})).unwrap();
    assert!(invoke(&window,"editor_stat_bounded",json!({"path":path})).is_err());
    assert!(invoke(&window,"editor_read_bounded",json!({"path":path})).is_err());
    assert!(invoke(&window,"editor_write_bounded",json!({"path":path,"content":"","create":false})).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(),size);
    let mut file=std::fs::File::open(&path).unwrap();
    let mut marker=[0;7]; file.read_exact(&mut marker).unwrap(); assert_eq!(&marker,b"R7-HEAD");
    file.seek(SeekFrom::End(-7)).unwrap(); file.read_exact(&mut marker).unwrap(); assert_eq!(&marker,b"R7-TAIL");
}
