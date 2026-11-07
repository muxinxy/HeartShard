//! 牧心后台库层：配置/认证/ACL/API 路由（CLI 入口在 main.rs）。

pub mod acl;
pub mod api;
pub mod auth;
pub mod config;

use heart_core::library::HeartCore;
use std::sync::Mutex;

pub struct AppState {
    pub core: Mutex<HeartCore>,
    pub users: auth::UserStore,
    pub acl: Mutex<acl::AclStore>,
    pub cfg: config::ServerConfig,
}
