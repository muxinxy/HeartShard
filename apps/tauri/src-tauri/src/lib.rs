// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;
pub mod library;
pub mod storage_ftp;
pub mod storage_s3;
pub mod storage_webdav;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 数据目录在 setup 中解析；解析失败直接报错退出（无数据目录无法工作）
            let data_dir =
                commands::resolve_data_dir(app.handle()).expect("cannot resolve app data dir");
            app.manage(commands::SessionState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_status,
            commands::unlock,
            commands::lock,
            commands::import_files,
            commands::list_blobs,
            commands::export_blob,
            commands::verify_blob,
            commands::sync_now,
            commands::repair_blob,
            commands::storage_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
