//! Tauri 命令层：把 `library.rs` 的业务函数暴露给前端。
//! 错误统一转 String（M1）；会话状态在 `SessionState`（Mutex 持 HeartCore）。

use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};

use crate::library::{BlobEntry, HeartCore, ImportOutcome, LibError, VerifyReport};

/// 全局会话：进程内唯一 HeartCore。
pub struct SessionState {
    core: Mutex<HeartCore>,
}

impl SessionState {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self { core: Mutex::new(HeartCore::new(data_dir)) }
    }
}

fn err_str(e: LibError) -> String {
    // 日志脱敏纪律：不打印密钥/口令/分片内容；LibError 的 Display 已保证
    e.to_string()
}

#[derive(Serialize)]
pub struct SessionStatus {
    pub unlocked: bool,
}

#[tauri::command]
pub fn session_status(state: State<'_, SessionState>) -> SessionStatus {
    SessionStatus { unlocked: state.core.lock().unwrap().is_unlocked() }
}

#[tauri::command]
pub fn unlock(password: String, state: State<'_, SessionState>) -> Result<SessionStatus, String> {
    let mut core = state.core.lock().unwrap();
    core.unlock(&password).map_err(err_str)?;
    Ok(SessionStatus { unlocked: true })
}

#[tauri::command]
pub fn lock(state: State<'_, SessionState>) -> SessionStatus {
    state.core.lock().unwrap().lock();
    SessionStatus { unlocked: false }
}

#[tauri::command]
pub fn import_files(
    paths: Vec<String>,
    state: State<'_, SessionState>,
) -> Result<Vec<ImportOutcome>, String> {
    let mut core = state.core.lock().unwrap();
    let mut results = Vec::with_capacity(paths.len());
    for p in &paths {
        results.push(core.import_file(p).map_err(err_str)?);
    }
    Ok(results)
}

#[tauri::command]
pub fn list_blobs(state: State<'_, SessionState>) -> Result<Vec<BlobEntry>, String> {
    state.core.lock().unwrap().list_blobs().map_err(err_str)
}

#[tauri::command]
pub fn export_blob(
    blob_hash: String,
    dest_path: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state
        .core
        .lock()
        .unwrap()
        .export_blob(&blob_hash, &dest_path)
        .map_err(err_str)
}

#[tauri::command]
pub fn verify_blob(blob_hash: String, state: State<'_, SessionState>) -> Result<VerifyReport, String> {
    state.core.lock().unwrap().verify_blob(&blob_hash).map_err(err_str)
}

/// 构造生产环境的数据目录（app data dir）。
pub fn resolve_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    Ok(dir)
}
