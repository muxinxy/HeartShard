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
    pub(crate) core: Mutex<HeartCore>,
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

// ---------------- §10/§11 补充：缩略图 / 详情 / 一键体检 / 历史报告 ----------------

#[tauri::command]
pub fn thumbnail(blob_hash: String, state: State<'_, SessionState>) -> Result<String, String> {
    state
        .core
        .lock()
        .unwrap()
        .thumbnail_cached(&blob_hash)
        .map_err(err_str)?
        .ok_or_else(|| "thumbnail not available".to_string())
}

#[tauri::command]
pub fn blob_detail(blob_hash: String, state: State<'_, SessionState>) -> Result<crate::library::BlobDetail, String> {
    state.core.lock().unwrap().blob_detail(&blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn verify_all(state: State<'_, SessionState>) -> Result<Vec<VerifyReport>, String> {
    state.core.lock().unwrap().verify_all().map_err(err_str)
}

#[tauri::command]
pub fn import_history(state: State<'_, SessionState>) -> Result<Vec<String>, String> {
    state.core.lock().unwrap().import_history().map_err(err_str)
}

#[tauri::command]
pub fn import_report_get(
    file: String,
    state: State<'_, SessionState>,
) -> Result<crate::library::ImportRunReport, String> {
    state.core.lock().unwrap().import_report_get(&file).map_err(err_str)
}

// ---------------- §10 设置页 / 相册树 / Range ----------------

#[tauri::command]
pub fn get_settings(state: State<'_, SessionState>) -> Result<crate::library::Settings, String> {
    Ok(state.core.lock().unwrap().settings().clone())
}

#[tauri::command]
pub fn update_settings(
    settings: crate::library::Settings,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state.core.lock().unwrap().set_settings(settings).map_err(err_str)
}

#[tauri::command]
pub fn change_password(
    old_password: String,
    new_password: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state
        .core
        .lock()
        .unwrap()
        .change_password(&old_password, &new_password)
        .map_err(err_str)
}

#[tauri::command]
pub fn list_albums(state: State<'_, SessionState>) -> Result<Vec<crate::library::AlbumInfo>, String> {
    state.core.lock().unwrap().list_albums().map_err(err_str)
}

#[tauri::command]
pub fn album_create(
    name: String,
    parent_id: Option<String>,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state.core.lock().unwrap().album_create(&name, parent_id).map_err(err_str)
}

#[tauri::command]
pub fn album_rename(
    album_id: String,
    name: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state.core.lock().unwrap().album_rename(&album_id, &name).map_err(err_str)
}

#[tauri::command]
pub fn album_set_sort(
    album_id: String,
    sort_order: i64,
    parent_id: Option<String>,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state
        .core
        .lock()
        .unwrap()
        .album_set_sort(&album_id, sort_order, parent_id)
        .map_err(err_str)
}

#[tauri::command]
pub fn album_set_cover(
    album_id: String,
    blob_hash: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state.core.lock().unwrap().album_set_cover(&album_id, &blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn export_blob_range(
    blob_hash: String,
    offset: u64,
    len: u64,
    state: State<'_, SessionState>,
) -> Result<Vec<u8>, String> {
    state
        .core
        .lock()
        .unwrap()
        .export_blob_range(&blob_hash, offset, len)
        .map_err(err_str)
}

#[tauri::command]
pub fn thumbnail_large(
    blob_hash: String,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state
        .core
        .lock()
        .unwrap()
        .thumbnail_large(&blob_hash)
        .map_err(err_str)
}

#[tauri::command]
pub fn storage_status(state: State<'_, SessionState>) -> Result<Vec<StorageStatus>, String> {
    state.core.lock().unwrap().storage_status().map_err(err_str)
}

#[tauri::command]
pub fn add_storage_local(
    id: String,
    root: String,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state
        .core
        .lock()
        .unwrap()
        .add_storage_local(&id, &root)
        .map_err(err_str)
}

#[tauri::command]
pub fn add_storage_s3(
    config: crate::storage_s3::S3Config,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state.core.lock().unwrap().add_storage_s3(config).map_err(err_str)
}

#[tauri::command]
pub fn add_storage_openlist(
    config: crate::storage_openlist::OpenListConfig,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state.core.lock().unwrap().add_storage_openlist(config).map_err(err_str)
}

#[tauri::command]
pub fn add_storage_webdav(
    config: crate::storage_webdav::WebDavConfig,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state.core.lock().unwrap().add_storage_webdav(config).map_err(err_str)
}

#[tauri::command]
pub fn add_storage_ftp(
    config: crate::storage_ftp::FtpConfig,
    state: State<'_, SessionState>,
) -> Result<String, String> {
    state.core.lock().unwrap().add_storage_ftp(config).map_err(err_str)
}

#[tauri::command]
pub fn remove_storage(id: String, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().remove_storage(&id).map_err(err_str)
}

#[tauri::command]
pub fn list_album_blobs(
    album_id: String,
    state: State<'_, SessionState>,
) -> Result<Vec<crate::library::BlobEntry>, String> {
    state.core.lock().unwrap().list_album_blobs(&album_id).map_err(err_str)
}

#[tauri::command]
pub fn album_add_blob(
    album_id: String,
    blob_hash: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state.core.lock().unwrap().album_add_blob(&album_id, &blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn album_remove_blob(
    album_id: String,
    blob_hash: String,
    state: State<'_, SessionState>,
) -> Result<(), String> {
    state.core.lock().unwrap().album_remove_blob(&album_id, &blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn rename_blob(blob_hash: String, new_name: String, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().rename_blob(&blob_hash, &new_name).map_err(err_str)
}

#[tauri::command]
pub fn set_blob_favorite(blob_hash: String, favorite: bool, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().set_blob_favorite(&blob_hash, favorite).map_err(err_str)
}

#[tauri::command]
pub fn delete_blob(blob_hash: String, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().delete_blob(&blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn restore_blob(blob_hash: String, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().restore_blob(&blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn purge_blob(blob_hash: String, state: State<'_, SessionState>) -> Result<(), String> {
    state.core.lock().unwrap().purge_blob(&blob_hash).map_err(err_str)
}

#[tauri::command]
pub fn list_trashed(state: State<'_, SessionState>) -> Result<Vec<crate::library::BlobEntry>, String> {
    state.core.lock().unwrap().list_trashed().map_err(err_str)
}

/// 系统默认程序打开文件/目录（Rust 侧调用，不经 capability 校验）。
#[tauri::command]
pub fn open_path_system(path: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("open path failed: {e}"))
}

/// §10 点击预览：解密 blob → 临时文件 → 返回 (路径, MIME)。
#[tauri::command]
pub fn preview_blob(
    blob_hash: String,
    state: State<'_, SessionState>,
) -> Result<PreviewResult, String> {
    let core = state.core.lock().unwrap();
    let (path, mime) = core.preview_blob(&blob_hash).map_err(err_str)?;
    Ok(PreviewResult { path: path.to_string_lossy().to_string(), mime })
}

#[derive(Serialize)]
pub struct PreviewResult {
    pub path: String,
    pub mime: String,
}

/// 构造数据目录：便携模式 → exe 同目录 data/（无 app_data_dir 依赖）。
pub fn resolve_data_dir(_app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot resolve exe path: {e}"))?;
    let dir = exe
        .parent()
        .ok_or("exe has no parent")?
        .join("data");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    Ok(dir)
}
