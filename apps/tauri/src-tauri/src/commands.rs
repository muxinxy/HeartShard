//! Tauri 命令层：把 `library.rs` 的业务函数暴露给前端。
//! 错误统一转 String（M1）；会话状态在 `SessionState`（Mutex 持 HeartCore）。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::library::{
    BlobEntry, HeartCore, ImportOutcome, LibError, RebalanceReport, RepairReport, StorageStatus,
    SyncOutcome, VerifyReport,
};
use crate::service_plugin::MobileService;

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

#[tauri::command]
pub fn sync_now(state: State<'_, SessionState>) -> Result<SyncOutcome, String> {
    state.core.lock().unwrap().sync_now().map_err(err_str)
}

/// 后台传输前台服务（M3）：批量导入/同步前开启，结束后关闭。
/// Android 上持续通知防止系统回收进程；桌面端静默成功。
#[tauri::command]
pub fn service_start(service: State<'_, MobileService>) -> Result<(), String> {
    service.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn service_stop(service: State<'_, MobileService>) -> Result<(), String> {
    service.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn repair_blob(blob_hash: String, state: State<'_, SessionState>) -> Result<RepairReport, String> {
    state.core.lock().unwrap().repair_blob(&blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn rebalance_all(state: State<'_, SessionState>) -> Result<RebalanceReport, String> {
    state.core.lock().unwrap().rebalance_all().map_err(err_str)
}

// ---------------- 目录导入五阶段向导（§8.5） ----------------

/// 全局取消标志（一次一个导入会话）。
pub struct ImportCancel(pub AtomicBool);

#[tauri::command]
pub fn import_scan_dir(
    dir: String,
    create_empty_albums: bool,
    state: State<'_, SessionState>,
) -> Result<crate::library::DirScanResult, String> {
    state
        .core
        .lock()
        .unwrap()
        .import_scan_dir(Path::new(&dir), create_empty_albums)
        .map_err(err_str)
}

#[tauri::command]
pub fn import_preview(
    session_id: String,
    state: State<'_, SessionState>,
) -> Result<crate::library::ImportPreview, String> {
    state.core.lock().unwrap().import_preview(&session_id).map_err(err_str)
}

#[tauri::command]
pub fn import_execute(
    app: tauri::AppHandle,
    session_id: String,
    state: State<'_, SessionState>,
    cancel: State<'_, ImportCancel>,
) -> Result<crate::library::ImportRunReport, String> {
    cancel.0.store(false, Ordering::Relaxed);
    let core = state.core.lock().unwrap();
    let result = core.import_execute(
        &session_id,
        crate::library::ImportOptions::default(),
        &cancel.0,
        &|progress| {
            // 两级进度经 Tauri event 推送（§8.5）
            let _ = app.emit("import-progress", &progress);
        },
    );
    result.map_err(err_str)
}

#[tauri::command]
pub fn import_cancel(cancel: State<'_, ImportCancel>) {
    cancel.0.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub fn storage_status(state: State<'_, SessionState>) -> Result<Vec<StorageStatus>, String> {
    state.core.lock().unwrap().storage_status().map_err(err_str)
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
