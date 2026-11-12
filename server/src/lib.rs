//! 牧心后台库层：配置/认证/ACL/API 路由（CLI 入口在 main.rs）。

pub mod acl;
pub mod admin;
pub mod api;
pub mod auth;
pub mod config;
pub mod meta;
pub mod share;
pub mod storage;
pub mod totp;

use heart_core::library::HeartCore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub core: Mutex<HeartCore>,
    pub users: auth::UserStore,
    pub acl: acl::AclStore,
    pub cfg: config::ServerConfig,
    /// server.toml 所在目录（share_cache 根）
    pub server_root: PathBuf,
    pub shares: share::ShareStore,
    /// 同步源（运行时管理，存元数据库，admin API 热更新）
    pub sources: Mutex<Vec<syncer::SourceConfig>>,
}

impl AppState {
    /// 元数据库句柄（admin API 写入用）。
    pub fn meta(&self) -> Arc<dyn meta::MetaStore> {
        self.users.meta_handle()
    }
}

impl AppState {
    /// 分享密文缓存目录（按相册 id 前 8 位分目录）。
    pub fn share_cache_dir(&self, album_prefix: &str) -> PathBuf {
        self.server_root.join("share_cache").join(album_prefix)
    }
}
