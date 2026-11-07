//! HeartShard 平台无关核心库（M1 抽库，原 apps/tauri/src-tauri/src 中的
//! library.rs 与四个存储适配器整体迁移至此）。
//!
//! 使用方：
//! - apps/tauri（桌面/安卓薄壳，re-export 本 crate）
//! - 牧心后台 / syncer CLI（M2/M3）
//!
//! 与 Tauri 类型完全解耦；Tauri 特有部分（commands.rs / storage_video.rs /
//! service_plugin.rs）留在 apps/tauri/src-tauri。

pub mod library;
pub mod storage_ftp;
pub mod storage_openlist;
pub mod storage_s3;
pub mod storage_webdav;

pub use library::*;
