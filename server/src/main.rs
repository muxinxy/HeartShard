//! 牧心后台（M3）CLI：启动解锁共享库 → 用户/组/ACL 初始化 → 常驻同步引擎 → HTTP 服务。
//!
//! ```text
//! heartshard-server [--config server.toml]
//! ```

use heart_core::library::HeartCore;
use heartshard_server::{acl, admin, api, auth::UserStore, config::ServerConfig, share, AppState};
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

    // 解锁共享库
    let password = match std::env::var(&cfg.library.password_env) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("错误：环境变量 {} 未设置（库解锁口令）", cfg.library.password_env);
            std::process::exit(2);
        }
    };
    let mut core = HeartCore::new(&cfg.library.dir);
    if let Err(e) = core.unlock(&password) {
        eprintln!("错误：库解锁失败（{}）：{e}", cfg.library.dir);
        std::process::exit(2);
    }

    let users = match UserStore::init(&server_root, &cfg) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(2);
        }
    };
    let acl_store = acl::AclStore::load(&server_root);
    let shares = share::ShareStore::load(&server_root);
    // 同步源运行时化：sources.json 优先；无文件时从 server.toml 迁移（写回文件）
    let sources = if server_root.join("sources.json").exists() {
        admin::load_sources(&server_root)
    } else {
        let s = cfg.sources.clone();
        let _ = std::fs::write(
            server_root.join("sources.json"),
            serde_json::to_vec_pretty(&s).unwrap_or_default(),
        );
        s
    };
    let state = Arc::new(AppState {
        core: Mutex::new(core),
        users: Mutex::new(users),
        acl: Mutex::new(acl_store),
        cfg: cfg.clone(),
        server_root,
        shares: Mutex::new(shares),
        sources: Mutex::new(sources),
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

        let app = api::router().with_state(state);
        let addr = format!("0.0.0.0:{}", cfg.port);
        eprintln!("牧心后台监听 http://{addr}");
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
        axum::serve(listener, app).await.expect("serve");
    });
}
