//! 牧心后台配置（server.toml）。
//!
//! 安全模型（家庭相册定位，如实声明）：
//! - **单一共享库**：后台启动时用 `password_env` 解锁一个相册库（一个 MK）。
//!   成员登录账户仅做认证与 ACL；ACL 是**应用层强制**（后台按权限过滤 API
//!   返回），不是密码学隔离——能直连底层存储的人可以解出全部照片。
//!   需要密码学隔离的场景属 M5「一人一库私密空间」。
//! - 登录口令 hash（argon2id + 随机盐）存 users.json，与库的 KEK 派生无关。

use serde::Deserialize;
use syncer::SourceConfig;

/// server.toml。
#[derive(Deserialize, Clone)]
pub struct ServerConfig {
    /// 监听端口（默认 8420）
    #[serde(default = "default_port")]
    pub port: u16,
    /// 相册库数据目录（mk.wrap / index / storages 同桌面版 data/）
    pub library: LibraryConfig,
    /// 成员账户（启动时写入/更新 users.json）
    #[serde(default, alias = "user")]
    pub users: Vec<UserEntry>,
    /// 自定义组（默认自动建 family 全员组，含全部用户）
    #[serde(default)]
    pub groups: Vec<GroupEntry>,
    /// 同步源（复用 syncer 配置结构；strategy/delete_mode 语义一致）
    #[serde(default, alias = "source")]
    pub sources: Vec<SourceConfig>,
    /// 同步轮询间隔（秒；0 = 不自动同步，仅手动触发）
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u64,
}

fn default_port() -> u16 {
    8420
}
fn default_sync_interval() -> u64 {
    300
}

#[derive(Deserialize, Clone)]
pub struct LibraryConfig {
    pub dir: String,
    /// 解锁口令的环境变量名（避免明文落盘）
    pub password_env: String,
}

#[derive(Deserialize, Clone)]
pub struct UserEntry {
    pub name: String,
    /// 登录口令（明文，来自 toml——首次启动后建议改用 password_env）
    pub password: Option<String>,
    /// 登录口令的环境变量名（优先于 password）
    pub password_env: Option<String>,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Deserialize, Clone)]
pub struct GroupEntry {
    pub name: String,
    /// 成员用户名列表
    #[serde(default)]
    pub members: Vec<String>,
}

impl ServerConfig {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("读取配置失败 {}: {e}", path.display()))?;
        toml::from_str(&raw).map_err(|e| format!("配置解析失败: {e}"))
    }
}
