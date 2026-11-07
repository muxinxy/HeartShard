//! HeartShard 相册同步工具（M2）CLI。
//!
//! 用法：
//! ```text
//! heartshard-syncer run    --config syncer.toml [--source id] [--full] [--watch]
//! heartshard-syncer export --config syncer.toml --album "2024旅行" --to D:\backup
//! ```
//!
//! 同步语义见 docs/development.md 同步工具章节。

use clap::{Parser, Subcommand};
use heartshard_syncer::{engine, export, load_config, unlock_library};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "heartshard-syncer", version, about = "牧心相册同步工具")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// 配置文件路径（默认 ./syncer.toml）
    #[arg(long, global = true, default_value = "syncer.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Cmd {
    /// 执行同步（默认一次性；--watch 常驻轮询）
    Run {
        /// 只跑指定 id 的源
        #[arg(long)]
        source: Option<String>,
        /// 忽略清单强制完全对账（修复清单漂移）
        #[arg(long)]
        full: bool,
        /// 常驻模式：每轮后等待 interval 秒再跑
        #[arg(long)]
        watch: bool,
        /// watch 轮询间隔（秒）
        #[arg(long, default_value_t = 300)]
        interval: u64,
    },
    /// 导出镜像：相册树 → 普通目录（按相册层级落盘，原图还原）
    Export {
        /// 相册名（按名称查找；导出该相册及全部子孙）
        #[arg(long)]
        album: String,
        /// 导出目标目录
        #[arg(long)]
        to: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let cfg = match load_config(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            std::process::exit(2);
        }
    };

    let code = match cli.cmd {
        Cmd::Run {
            source,
            full,
            watch,
            interval,
        } => match run_all(&cfg, source.as_deref(), full) {
            Ok(_) => {
                if watch {
                    loop {
                        eprintln!("--watch：{interval} 秒后进入下一轮");
                        std::thread::sleep(std::time::Duration::from_secs(interval));
                        if let Err(e) = run_all(&cfg, source.as_deref(), false) {
                            eprintln!("本轮同步失败：{e}");
                        }
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("同步失败：{e}");
                1
            }
        },
        Cmd::Export { album, to } => match export::run_export(&cfg, &album, &to) {
            Ok(n) => {
                println!("已导出 {n} 张到 {}", to.display());
                0
            }
            Err(e) => {
                eprintln!("导出失败：{e}");
                1
            }
        },
    };
    std::process::exit(code);
}

fn run_all(cfg: &heartshard_syncer::Config, only: Option<&str>, full: bool) -> Result<(), String> {
    let mut core = unlock_library(cfg)?;
    let (mut total_new, mut total_moved, mut total_removed) = (0, 0, 0);
    for src in &cfg.source {
        if let Some(id) = only {
            if src.id != id {
                continue;
            }
        }
        let report = engine::sync_source(&mut core, src, full)?;
        println!(
            "[{}] 新增 {} · 移动 {} · 删除 {} （策略 {}）",
            src.id,
            report.added,
            report.moved,
            report.removed,
            if src.strategy == "mirror" { "mirror" } else { "append" }
        );
        total_new += report.added;
        total_moved += report.moved;
        total_removed += report.removed;
    }
    // 末尾统一发布增量（空 delta 内部会自动跳过）
    core.sync_now().map_err(|e| format!("增量发布失败: {e}"))?;
    println!("完成：新增 {total_new} · 移动 {total_moved} · 删除 {total_removed}");
    Ok(())
}
