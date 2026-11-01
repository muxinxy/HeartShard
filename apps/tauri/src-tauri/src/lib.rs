// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;
pub mod library;
pub mod service_plugin;
pub mod storage_ftp;
pub mod storage_openlist;
pub mod storage_s3;
pub mod storage_webdav;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            // M3 后台传输：Android 前台服务插件（Kotlin 侧在 gen/android）。
            // 桌面端 setup 为空操作，命令静默成功。
            tauri::plugin::Builder::new("heartshard-service")
                .setup(|app, api: tauri::plugin::PluginApi<_, serde_json::Value>| {
                    #[cfg(mobile)]
                    {
                        let service = service_plugin::init(app, api)?;
                        app.manage(service_plugin::MobileService {
                            inner: Some(service_plugin::MobileServiceInner(service)),
                        });
                    }
                    #[cfg(desktop)]
                    {
                        app.manage(service_plugin::MobileService { inner: None });
                    }
                    Ok(())
                })
                .build(),
        )
        .setup(|app| {
            // 数据目录在 setup 中解析；解析失败直接报错退出（无数据目录无法工作）
            let data_dir =
                commands::resolve_data_dir(app.handle()).expect("cannot resolve app data dir");
            app.manage(commands::SessionState::new(data_dir));
            app.manage(commands::ImportCancel(std::sync::atomic::AtomicBool::new(false)));
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
            commands::rebalance_all,
            commands::storage_status,
            commands::service_start,
            commands::service_stop,
            commands::import_scan_dir,
            commands::import_preview,
            commands::import_execute,
            commands::import_cancel,
            commands::thumbnail,
            commands::blob_detail,
            commands::verify_all,
            commands::import_history,
            commands::import_report_get,
            commands::get_settings,
            commands::update_settings,
            commands::change_password,
            commands::list_albums,
            commands::album_create,
            commands::album_rename,
            commands::album_set_sort,
            commands::album_set_cover,
            commands::export_blob_range,
            commands::thumbnail_large,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
