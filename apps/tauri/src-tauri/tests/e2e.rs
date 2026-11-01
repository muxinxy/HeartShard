//! M2 退出标准 E2E：
//! 1. 单设备：导入 → 双 Storage 分片/manifest 冗余 → 体检 → 去重 → 还原 → SHA256；
//! 2. 双设备共享云端模拟：时间线一致 + Lamport 裁决（相册冲突在 core-rs 单测覆盖）；
//! 3. 错口令在 unlock 即失败（SQLCipher）。

use tauri_lib::library::{HeartCore, ImportStatus, LibError};

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

fn count_files(root: &std::path::Path) -> usize {
    walk_all(root).len()
}

fn walk_all(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    for e in entries.flatten() {
        if e.path().is_dir() {
            out.extend(walk_all(&e.path()));
        } else {
            out.push(e.path());
        }
    }
    out
}

fn setup_shared_storages(core: &mut HeartCore, clouds: &[&std::path::Path]) {
    // 移除默认本地 Storage，改用共享"云"（模拟两台设备连同一组 Storage）
    for id in ["local-a", "local-b"] {
        let _ = core.remove_storage(id);
    }
    for (i, root) in clouds.iter().enumerate() {
        core.add_storage_local(&format!("cloud-{}", i + 1), root).unwrap();
    }
}

#[test]
fn single_device_shard_manifest_redundancy_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("correct horse battery staple").unwrap();

    // 2.5 MiB 源文件（3 chunk）
    let src_dir = tempfile::tempdir().unwrap();
    let src_path = src_dir.path().join("sunset_beach_2026.jpg");
    let blob = random_bytes(0xA11CE, 2 * 1_048_576 + 12345);
    std::fs::write(&src_path, &blob).unwrap();

    let outcome = core.import_file(&src_path).unwrap();
    assert_eq!(outcome.status, ImportStatus::Imported, "{outcome:?}");
    let blob_hash = outcome.blob_hash.clone().unwrap();

    // 冗余体检：全部 shard 健康
    let report = core.verify_blob(&blob_hash).unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.shard_count, 3 * 4);

    // shard 分散到默认双 Storage；manifest 在两个 Storage 各一份（§6.4）
    let st = core.storage_status().unwrap();
    assert_eq!(st.len(), 2);
    let blobs_a = count_files(&dir.path().join("storages").join("local-a"));
    let blobs_b = count_files(&dir.path().join("storages").join("local-b"));
    assert_eq!(blobs_a + blobs_b, 12 + 2, "12 shards + 2 manifest copies");

    // 列表
    let blobs = core.list_blobs().unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].file_name, "sunset_beach_2026.jpg");

    // 去重
    let again = core.import_file(&src_path).unwrap();
    assert_eq!(again.status, ImportStatus::SkippedDuplicate);

    // 还原
    let out_path = dir.path().join("restored.jpg");
    core.export_blob(&blob_hash, &out_path).unwrap();
    assert_eq!(std::fs::read(&out_path).unwrap(), blob);

    // 单点损失：删掉任一 shard 后仍可还原
    let all = walk_all(&dir.path().join("storages"));
    let shard_file = all
        .iter()
        .find(|p| p.extension().map(|e| e == "shard").unwrap_or(false))
        .unwrap();
    std::fs::remove_file(shard_file).unwrap();
    let after = core.verify_blob(&blob_hash).unwrap();
    assert_eq!(after.missing.len(), 1);
    assert!(!after.ok);
    let out2 = dir.path().join("restored2.jpg");
    core.export_blob(&blob_hash, &out2).unwrap();
    assert_eq!(std::fs::read(&out2).unwrap(), blob);

    // 锁定
    core.lock();
    assert!(matches!(core.list_blobs(), Err(LibError::Locked)));
}

#[test]
fn two_devices_sync_converge_timelines() {
    let cloud1 = tempfile::tempdir().unwrap();
    let cloud2 = tempfile::tempdir().unwrap();
    let dev_a = tempfile::tempdir().unwrap();
    let dev_b = tempfile::tempdir().unwrap();

    let mut a = HeartCore::new(dev_a.path());
    a.unlock("album-pass").unwrap();
    setup_shared_storages(&mut a, &[cloud1.path(), cloud2.path()]);
    let mut b = HeartCore::new(dev_b.path());
    b.unlock("album-pass").unwrap();
    setup_shared_storages(&mut b, &[cloud1.path(), cloud2.path()]);

    // A 导入照片并同步发布
    let src1 = tempfile::tempdir().unwrap();
    let f1 = src1.path().join("a_photo.png");
    let blob1 = random_bytes(0xBEEF, 1_048_576 + 321);
    std::fs::write(&f1, &blob1).unwrap();
    let o1 = a.import_file(&f1).unwrap();
    assert_eq!(o1.status, ImportStatus::Imported);
    let hash1 = o1.blob_hash.clone().unwrap();
    let rep_a = a.sync_now().unwrap();
    // 导入时钟 +1、发布再 +1 → 2
    assert_eq!(rep_a.published_lamport, 2);
    assert_eq!(rep_a.stores_ok.len(), 2, "publish to both clouds");

    // B 同步 → 拉到 A 的 blob
    let rep_b1 = b.sync_now().unwrap();
    assert_eq!(rep_b1.applied_objects, 1);
    assert_eq!(rep_b1.published_lamport, 3, "fold A's clock(2) then bump");
    assert_eq!(b.list_blobs().unwrap().len(), 1);

    // B 导入自己的照片并发布
    let src2 = tempfile::tempdir().unwrap();
    let f2 = src2.path().join("b_photo.jpg");
    let blob2 = random_bytes(0xCAFE, 524_288 + 7);
    std::fs::write(&f2, &blob2).unwrap();
    let o2 = b.import_file(&f2).unwrap();
    let hash2 = o2.blob_hash.clone().unwrap();
    assert_ne!(hash1, hash2);
    b.sync_now().unwrap();

    // A 再同步 → 时间线与 B 完全一致（M2 退出标准）
    a.sync_now().unwrap();
    let timeline_a = a.list_blobs().unwrap();
    let timeline_b = b.list_blobs().unwrap();
    assert_eq!(timeline_a.len(), 2);
    assert_eq!(timeline_a, timeline_b, "timelines must converge");

    // B 可以还原 A 的照片（shard/manifest 均在共享 Storage）
    let out = dev_b.path().join("restored_from_a.png");
    b.export_blob(&hash1, &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), blob1);

    // A 也能还原 B 的照片
    let out2 = dev_a.path().join("restored_from_b.jpg");
    a.export_blob(&hash2, &out2).unwrap();
    assert_eq!(std::fs::read(&out2).unwrap(), blob2);

    // 收敛检查：全量发布下，再拉一轮后即无新对象（内容早已一致）
    b.sync_now().unwrap();
    let rep_b4 = b.sync_now().unwrap();
    assert_eq!(rep_b4.applied_objects, 0);
    assert_eq!(b.list_blobs().unwrap(), timeline_b);
}

#[test]
fn repair_restores_missing_shard() {
    // M5 修复任务 E2E：删 shard → repair → 体检恢复健康 → 导出逐字节一致
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("repair-pass").unwrap();

    let src = dir.path().join("photo.png");
    let blob = random_bytes(0x5EED, 1_048_576 + 999);
    std::fs::write(&src, &blob).unwrap();
    let outcome = core.import_file(&src).unwrap();
    let blob_hash = outcome.blob_hash.clone().unwrap();

    // 删除一个 shard（模拟存储事故）
    let all = walk_all(&dir.path().join("storages"));
    let shard_file = all
        .iter()
        .find(|p| p.extension().map(|e| e == "shard").unwrap_or(false))
        .unwrap();
    std::fs::remove_file(shard_file).unwrap();
    assert!(!core.verify_blob(&blob_hash).unwrap().ok);

    // 修复 → 健康
    let report = core.repair_blob(&blob_hash).unwrap();
    assert_eq!(report.repaired.len(), 1, "{report:?}");
    assert!(report.unrecoverable.is_empty());
    let after = core.verify_blob(&blob_hash).unwrap();
    assert!(after.ok, "{after:?}");

    // 修复后导出仍逐字节一致
    let out = dir.path().join("after_repair.png");
    core.export_blob(&blob_hash, &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), blob);
}

#[test]
fn wrong_password_fails_at_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("right-password").unwrap();
    let src = dir.path().join("a.png");
    std::fs::write(&src, random_bytes(7, 4096)).unwrap();
    core.import_file(&src).unwrap();
    core.lock();

    // SQLCipher：错口令在打开索引时即失败
    let err = core.unlock("wrong-password").unwrap_err();
    assert!(
        err.to_string().contains("wrong password"),
        "unlock must fail on wrong password, got {err}"
    );
    assert!(!core.is_unlocked());

    // 正确口令仍可解锁
    core.unlock("right-password").unwrap();
    assert_eq!(core.list_blobs().unwrap().len(), 1);
}
