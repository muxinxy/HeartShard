//! keypath 跨端一致性校验器（Rust 参考实现侧）。
//!
//! 用法：`cargo run -p keypath-tests`（或 `cargo run --manifest-path tools/keypath-tests/Cargo.toml`）
//! 读取同目录 `vectors.json`，逐条断言 Rust 实现输出与向量一致。
//! 其他端（ArkTS/JS）实现后，应产出与 `vectors.json` 相同的结果并以本工具输出对照。

use std::path::Path;

fn main() {
    let vectors_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors.json");
    let raw = std::fs::read_to_string(&vectors_path).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", vectors_path.display());
    });
    let vectors: serde_json::Value = serde_json::from_str(&raw).expect("vectors.json must be valid JSON");

    let mut passed = 0usize;
    let mut failed = 0usize;

    for case in vectors["sanitize"].as_array().expect("sanitize array") {
        let input = case["input"].as_str().expect("input str");
        let valid = case["valid"].as_bool().expect("valid bool");
        let ok = keypath::sanitize_key_segment(input).is_ok();
        if ok == valid {
            passed += 1;
        } else {
            failed += 1;
            eprintln!("FAIL sanitize({input:?}): got valid={ok}, expected {valid}");
        }
    }

    for case in vectors["shard_key"].as_array().expect("shard_key array") {
        let expected = case["expected"].as_str().unwrap();
        let got = keypath::shard_object_key(
            case["blob_hash"].as_str().unwrap(),
            case["chunk_index"].as_u64().unwrap() as u32,
            case["shard_index"].as_u64().unwrap() as u8,
        );
        match got {
            Ok(k) if k == expected => passed += 1,
            other => {
                failed += 1;
                eprintln!("FAIL shard_key: got {other:?}, expected {expected:?}");
            }
        }
    }

    for case in vectors["manifest_key"].as_array().expect("manifest_key array") {
        let expected = case["expected"].as_str().unwrap();
        match keypath::manifest_object_key(case["blob_hash"].as_str().unwrap()) {
            Ok(k) if k == expected => passed += 1,
            other => {
                failed += 1;
                eprintln!("FAIL manifest_key: got {other:?}, expected {expected:?}");
            }
        }
    }

    for case in vectors["index_key"].as_array().expect("index_key array") {
        let expected = case["expected"].as_str().unwrap();
        match keypath::index_object_key(
            case["device_id"].as_str().unwrap(),
            case["lamport_clock"].as_u64().unwrap(),
        ) {
            Ok(k) if k == expected => passed += 1,
            other => {
                failed += 1;
                eprintln!("FAIL index_key: got {other:?}, expected {expected:?}");
            }
        }
    }

    println!("keypath vectors: {passed} passed, {failed} failed");
    if failed > 0 {
        std::process::exit(1);
    }
}
