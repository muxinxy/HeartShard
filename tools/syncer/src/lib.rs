//! HeartShard 相册同步工具库（M2）：策略矩阵（append/mirror）、三层 diff、
//! 目录树映射相册、移动只改关联、导出镜像。CLI 与集成测试共用此 crate。

pub mod engine;
pub mod export;

use serde::Deserialize;

pub type Res<T> = Result<T, String>;

/// syncer.toml 配置。
#[derive(Deserialize)]
pub struct Config {
    pub data: DataConfig,
    #[serde(default)]
    pub source: Vec<SourceConfig>,
}

#[derive(Deserialize)]
pub struct DataConfig {
    /// 相册库数据目录（与桌面版 data/ 同构，含 mk.wrap/index/storages）
    pub dir: String,
    /// 解锁口令；未配置时读环境变量 HEARTSHARD_PASSWORD，再退化为终端输入
    pub password: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct SourceConfig {
    pub id: String,
    /// local | s3 | webdav | openlist
    pub kind: String,
    /// local 源的根目录
    pub root: Option<String>,
    /// append（默认，只新增）| mirror（镜像：修改进新版本、删除跟进）
    #[serde(default)]
    pub strategy: String,
    /// mirror 下的删除处理：keep（默认，相册保留）| trash（进回收站）
    #[serde(default)]
    pub delete_mode: String,
    /// 相册树挂载点名；空 = 直接用源内相对目录
    #[serde(default)]
    pub album_root: Option<String>,
    /// s3 = { endpoint, bucket, access_key, secret_key, region, path_style }
    pub s3: Option<heart_core::storage_s3::S3Config>,
    /// webdav = { base_url, username, password }
    pub webdav: Option<heart_core::storage_webdav::WebDavConfig>,
    /// openlist = { base_url, username, password, base_path }
    pub openlist: Option<heart_core::storage_openlist::OpenListConfig>,
}

pub fn load_config(path: &std::path::Path) -> Res<Config> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("读取配置失败 {}: {e}", path.display()))?;
    toml::from_str(&raw).map_err(|e| format!("配置解析失败: {e}"))
}

pub fn unlock_library(cfg: &Config) -> Res<heart_core::library::HeartCore> {
    use heart_core::library::HeartCore;
    let mut core = HeartCore::new(&cfg.data.dir);
    let password = match (&cfg.data.password, std::env::var("HEARTSHARD_PASSWORD")) {
        (Some(p), _) => p.clone(),
        (None, Ok(p)) => p,
        (None, Err(_)) => {
            eprint!("口令（解锁相册库 {}）：", cfg.data.dir);
            rpassword::read_password().map_err(|e| format!("读取口令失败: {e}"))?
        }
    };
    core.unlock(&password).map_err(|e| format!("解锁失败: {e}"))?;
    Ok(core)
}

/// 测试与 CLI 共用入口别名。
pub use engine::sync_source as sync_source_for_test;
