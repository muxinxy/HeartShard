//! 牧心后台（M3）CLI：启动解锁共享库 → 用户/组/ACL 初始化 → 常驻同步引擎 → HTTP 服务。
//!
//! ```text
//! heartshard-server [--config server.toml]
//! ```

use heart_core::library::HeartCore;
use heartshard_server::{acl, admin, api, auth::UserStore, config::ServerConfig, meta, share, AppState};
use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cli_cfg = match args.iter().position(|a| a == "--config") {
        Some(i) => args
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| "server.toml".into()),
        None => "server.toml".into(),
    };
    let cfg = match ServerConfig::load(std::path::Path::new(&cli_cfg)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(2);
        }
    };

    // 服务器数据文件（users/groups/acls.json）与 server.toml 同目录
    let server_root = std::path::Path::new(&cli_cfg)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 解锁共享库：环境变量口令；未设置时交互输入
    let password = match std::env::var(&cfg.library.password_env) {
        Ok(p) => p,
        Err(_) => {
            eprint!("口令（解锁相册库 {}）：", cfg.library.dir);
            use std::io::BufRead;
            let mut line = String::new();
            match std::io::stdin().lock().read_line(&mut line) {
                Ok(_) => line.trim().to_string(),
                Err(e) => {
                    eprintln!("错误：读取口令失败：{e}");
                    std::process::exit(2);
                }
            }
        }
    };
    let mut core = HeartCore::new(&cfg.library.dir);
    if let Err(e) = core.unlock(&password) {
        eprintln!("错误：库解锁失败（{}）：{e}", cfg.library.dir);
        std::process::exit(2);
    }

    // 元数据库（sqlite 默认 / mysql 可选），JSON 文件一次性迁移
    let meta_store = match meta::open_meta(&cfg, &server_root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(2);
        }
    };
    let users = match UserStore::init(meta_store.clone(), &cfg) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(2);
        }
    };
    let acl_store = acl::AclStore::new(meta_store.clone());
    let shares = share::ShareStore::new(meta_store.clone());
    // 同步源：数据库优先；空且 server.toml 有源则迁移写入
    let mut sources_db = meta_store.list_sources().unwrap_or_default();
    if sources_db.is_empty() && !cfg.sources.is_empty() {
        sources_db = cfg
            .sources
            .iter()
            .map(|s| serde_json::to_value(s).unwrap_or_default())
            .collect();
        let _ = meta_store.save_sources(&sources_db);
        eprintln!("[meta] server.toml sources -> 数据库（{} 个源）", sources_db.len());
    }
    let mut sources_runtime: Vec<syncer::SourceConfig> = sources_db
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    if sources_runtime.is_empty() {
        sources_runtime = cfg.sources.clone();
    }
    let state = Arc::new(AppState {
        core: Mutex::new(core),
        users,
        acl: acl_store,
        cfg: cfg.clone(),
        server_root,
        shares,
        sources: Mutex::new(sources_runtime),
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move {
        // 常驻同步引擎（复用 syncer 策略矩阵/清单 diff；源列表每轮从运行时配置读取）
        if cfg.sync_interval_secs > 0 {
            let sync_state = state.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        sync_state.cfg.sync_interval_secs,
                    ))
                    .await;
                    let sources = sync_state.sources.lock().unwrap().clone();
                    if sources.is_empty() {
                        continue;
                    }
                    let st = sync_state.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        for src in &sources {
                            let mut core = st.core.lock().unwrap();
                            match syncer::engine::sync_source(&mut core, src, false) {
                                Ok(r) => eprintln!(
                                    "[sync {}] 新增 {} · 移动 {} · 删除 {}",
                                    src.id, r.added, r.moved, r.removed
                                ),
                                Err(e) => eprintln!("[sync {}] 失败: {e}", src.id),
                            }
                        }
                        let core = st.core.lock().unwrap();
                        let _ = core.sync_now();
                    })
                    .await;
                }
            });
        }

        // CORS：桌面客户端（tauri://localhost）与其它源的浏览器页面是跨域 fetch；
        // API 由 token 保护，这里放行全部来源即可。
        let app = api::router()
            .with_state(state)
            .layer(tower_http::cors::CorsLayer::permissive());
        let addr = format!("0.0.0.0:{}", cfg.port);
        eprintln!("牧心后台监听 http://{addr}");
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
        axum::serve(listener, app).await.expect("serve");
    });
}
