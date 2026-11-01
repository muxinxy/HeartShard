//! S3 兼容存储连通性验收工具（简报 §9.3/§14：写适配器前先用最小脚本跑通 put/get）。
//!
//! 用法（pwsh）：
//!   $env:S3_ENDPOINT="https://s3.amazonaws.com"; $env:S3_BUCKET="my-bucket"
//!   $env:S3_ACCESS_KEY="..."; $env:S3_SECRET_KEY="..."
//!   $env:S3_REGION="us-east-1"; $env:S3_PATH_STYLE="1"   # MinIO/R2 置 1
//!   cargo run -p heartshard-tauri --bin s3-verify
//!
//! 流程：put → get（逐字节比对）→ list（前缀过滤）→ delete → 全部通过输出 PASS。

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_lib::storage_s3::{S3Config, S3Store};
use index::store::BlobStore;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing env {name}"))
}

fn main() {
    let config = S3Config {
        endpoint: env("S3_ENDPOINT"),
        bucket: env("S3_BUCKET"),
        access_key: env("S3_ACCESS_KEY"),
        secret_key: env("S3_SECRET_KEY"),
        region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        path_style: std::env::var("S3_PATH_STYLE").map(|v| v == "1").unwrap_or(true),
    };
    let store = S3Store::new(config).expect("cannot create S3 store");

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let prefix = format!("heartshard-verify/{ts}");
    let key = format!("{prefix}/probe.bin");
    let payload: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();

    let mut failed = 0;
    macro_rules! step {
        ($name:expr, $block:expr) => {
            match $block {
                Ok(()) => println!("PASS  {}", $name),
                Err(e) => {
                    println!("FAIL  {}: {e}", $name);
                    failed += 1;
                }
            }
        };
    }

    step!("put", store.put(&key, &payload).map_err(|e| format!("{e}")));
    step!("get byte-exact", {
        match store.get(&key) {
            Some(got) if got == payload => Ok(()),
            Some(got) => Err(format!("len mismatch: got {}, want {}", got.len(), payload.len())),
            None => Err("object not found after put".into()),
        }
    });
    step!("get missing returns none", {
        if store.get(&format!("{prefix}/nope.bin")).is_none() {
            Ok::<(), String>(())
        } else {
            Err("unexpected object".to_string())
        }
    });
    step!("list by prefix", {
        match store.list(&prefix) {
            Ok(keys) if keys.contains(&key) => Ok(()),
            Ok(keys) => Err(format!("list result missing {key}: {keys:?}")),
            Err(e) => Err(format!("{e}")),
        }
    });
    step!("delete", store.delete(&key).map_err(|e| format!("{e}")));
    step!("delete idempotent", store.delete(&key).map_err(|e| format!("{e}")));
    step!("get after delete is none", {
        if store.get(&key).is_none() {
            Ok::<(), String>(())
        } else {
            Err("object still exists after delete".to_string())
        }
    });

    if failed == 0 {
        println!("S3 VERIFY: ALL PASS ({})", store.id());
    } else {
        println!("S3 VERIFY: {failed} step(s) FAILED");
        std::process::exit(1);
    }
}
