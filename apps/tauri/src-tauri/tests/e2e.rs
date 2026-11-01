//! M1 退出标准 E2E：上传 → 分片到本地目录（模拟 Storage）→ 下载还原 → SHA256 校验。

use tauri_lib::library::{HeartCore, ImportStatus, LibError, MOCK_STORAGE_ID};

/// xorshift64 确定性伪随机（测试专用）。
struct XorShift(u64);

impl XorShift {
    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            let v = x.to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}

fn random_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = XorShift(seed | 1);
    let mut data = vec![0u8; len];
    rng.fill(&mut data);
    data
}

#[test]
fn m1_import_mock_storage_export_verify_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());

    // 锁定状态下操作必须被拒绝
    assert!(matches!(core.list_blobs(), Err(LibError::Locked)));

    // 1. 解锁（首次：生成 KDF 参数）
    core.unlock("correct horse battery staple").unwrap();

    // 2. 准备 2.5 MiB 源文件（3 个 chunk：2 满 + 1 短，覆盖末片路径）
    let src_dir = tempfile::tempdir().unwrap();
    let src_path = src_dir.path().join("sunset_beach_2026.jpg");
    let blob = random_bytes(0xA11CE, 2 * 1_048_576 + 12345);
    std::fs::write(&src_path, &blob).unwrap();

    // 3. 导入 → 分片写入本地目录模拟 Storage
    let outcome = core.import_file(&src_path).unwrap();
    assert_eq!(outcome.status, ImportStatus::Imported, "{outcome:?}");
    let blob_hash = outcome.blob_hash.clone().unwrap();

    // shard 对象真实存在于模拟 Storage，且键符合 keypath 规范
    let report = core.verify_blob(&blob_hash).unwrap();
    assert!(report.ok, "all shards must verify right after import: {report:?}");
    assert_eq!(report.shard_count, 3 * 4); // 3 chunks × n=4

    // 4. 列表（时间线雏形）
    let blobs = core.list_blobs().unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].blob_hash, blob_hash);
    assert_eq!(blobs[0].file_name, "sunset_beach_2026.jpg");
    assert_eq!(blobs[0].mime, "image/jpeg");

    // 5. 去重：同一文件再导入 → SkippedDuplicate（§8.4）
    let again = core.import_file(&src_path).unwrap();
    assert_eq!(again.status, ImportStatus::SkippedDuplicate);

    // 6. 下载还原 → SHA256 校验（源文件删除后从 Storage 还原）
    let out_path = dir.path().join("restored").join("sunset_beach_2026.jpg");
    core.export_blob(&blob_hash, &out_path).unwrap();
    let restored = std::fs::read(&out_path).unwrap();
    assert_eq!(restored.len(), blob.len());
    assert_eq!(restored, blob, "restored bytes must equal source byte-for-byte");

    // 7. 冗余容错：删掉 1 个 shard（模拟单点损失），n=4/m=2 仍可还原
    let root = dir.path().join("storages").join(MOCK_STORAGE_ID).join("blobs");
    let some_shard_key = walk_two_levels(&root)
        .first()
        .expect("at least one shard file exists")
        .clone();
    // some_shard_key 已是相对 data_dir 的完整路径（storages/local-mock/blobs/...）
    let shard_file = dir.path().join(&some_shard_key);
    assert!(shard_file.exists(), "walked shard must exist: {shard_file:?}");
    std::fs::remove_file(&shard_file).unwrap();
    let after_loss = core.verify_blob(&blob_hash).unwrap();
    assert_eq!(after_loss.missing.len(), 1, "verify must report the missing shard");
    assert!(!after_loss.ok);

    let out2 = dir.path().join("restored2.jpg");
    core.export_blob(&blob_hash, &out2).unwrap();
    assert_eq!(std::fs::read(&out2).unwrap(), blob, "export must survive one lost shard");

    // 8. 锁定后一切敏感操作被拒绝
    core.lock();
    assert!(matches!(core.list_blobs(), Err(LibError::Locked)));
    assert!(matches!(
        core.export_blob(&blob_hash, &out_path),
        Err(LibError::Locked)
    ));
}

#[test]
fn wrong_password_detected_on_index_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("right-password").unwrap();
    let src = dir.path().join("a.png");
    std::fs::write(&src, random_bytes(7, 4096)).unwrap();
    core.import_file(&src).unwrap();
    core.lock();

    // 错误口令：解锁成功（KDF 可计算），但索引解密必然失败 → 数据可检测
    core.unlock("wrong-password").unwrap();
    let err = core.list_blobs().unwrap_err();
    assert!(
        err.to_string().contains("decrypt failed"),
        "wrong password must surface as index decrypt failure, got {err}"
    );
}

/// 在 storages/blobs 下找第一个 shard 文件的完整 remote_key（两层桶 + blob 目录）。
fn walk_two_levels(root: &std::path::Path) -> Vec<String> {
    let mut keys = Vec::new();
    let Ok(buckets) = std::fs::read_dir(root) else { return keys };
    for b in buckets.flatten() {
        let Ok(subs) = std::fs::read_dir(b.path()) else { continue };
        for s in subs.flatten() {
            let Ok(blob_dirs) = std::fs::read_dir(s.path()) else { continue };
            for bd in blob_dirs.flatten() {
                let Ok(files) = std::fs::read_dir(bd.path()) else { continue };
                for f in files.flatten() {
                    let key = f
                        .path()
                        .strip_prefix(root.parent().unwrap().parent().unwrap().parent().unwrap())
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    keys.push(key);
                }
            }
        }
    }
    keys
}
