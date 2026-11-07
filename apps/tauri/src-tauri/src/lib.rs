// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;
// M1 抽库：HeartCore 与四个存储适配器已迁移至 heart-core crate（平台无关），
// 这里 re-export 保持 crate::library / crate::storage_* 路径不变（commands.rs、
// e2e 测试与 storage_video.rs 无需改动）。
pub use heart_core::{library, storage_ftp, storage_openlist, storage_s3, storage_webdav};
mod service_plugin;
mod storage_video;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("heartshard-video", |ctx, request| {
            storage_video::handle_video_request(&ctx, request)
        })
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
            commands::add_storage_local,
            commands::add_storage_s3,
            commands::add_storage_openlist,
            commands::add_storage_webdav,
            commands::add_storage_ftp,
            commands::remove_storage,
            commands::preview_blob,
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
            commands::list_album_blobs,
            commands::album_add_blob,
            commands::album_remove_blob,
            commands::rename_blob,
            commands::set_blob_favorite,
            commands::delete_blob,
            commands::restore_blob,
            commands::purge_blob,
            commands::list_trashed,
            commands::open_path_system,
            commands::export_blob_range,
            commands::thumbnail_large,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
