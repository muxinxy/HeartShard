//! E2E：M2 退出标准 + §12.2 目录导入场景（增量幂等 / 5000 文件 / 断网续传）。

use std::sync::atomic::AtomicBool;
use tauri_lib::library::{HeartCore, ImportOptions, ImportStatus, LibError};

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

/// 相册应用核心交互：重命名 / 收藏 / 相册内容 / 缩略图按需生成 / 回收站 / 清除碎片。
#[test]
fn photo_lifecycle_rename_favorite_trash_album() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("correct horse battery staple").unwrap();

    // 真实 PNG（缩略图生成需要可解码的图片）
    let src_dir = tempfile::tempdir().unwrap();
    let src_path = src_dir.path().join("pic.png");
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(320, 240)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    std::fs::write(&src_path, &png).unwrap();

    let outcome = core.import_file(&src_path).unwrap();
    assert_eq!(outcome.status, ImportStatus::Imported, "{outcome:?}");
    let blob_hash = outcome.blob_hash.clone().unwrap();

    // 重命名 + 收藏
    core.rename_blob(&blob_hash, "海边假日.png").unwrap();
    core.set_blob_favorite(&blob_hash, true).unwrap();
    let blobs = core.list_blobs().unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].file_name, "海边假日.png");
    assert!(blobs[0].favorite);

    // 相册加入 + 内容查询
    let album_id = core.album_create("旅行", None).unwrap();
    core.album_add_blob(&album_id, &blob_hash).unwrap();
    let album_blobs = core.list_album_blobs(&album_id).unwrap();
    assert_eq!(album_blobs.len(), 1);
    assert_eq!(album_blobs[0].blob_hash, blob_hash);

    // 256px 缩略图（导入期缓存或按需生成）
    assert!(core.thumbnail_cached(&blob_hash).unwrap().is_some());
    // 1024px 大图：缓存未命中 → 解密按需生成 → 落加密缓存
    let large = dir.path().join("thumbs").join(format!("{blob_hash}.1024.enc"));
    assert!(!large.exists());
    let url = core.thumbnail_large(&blob_hash).unwrap();
    assert!(url.starts_with("data:image/png;base64,"));
    assert!(large.exists(), "大图缓存应已生成");

    // 回收站：删除 → 主列表消失、回收站出现 → 恢复
    core.delete_blob(&blob_hash).unwrap();
    assert!(core.list_blobs().unwrap().is_empty());
    let trash = core.list_trashed().unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].blob_hash, blob_hash);
    core.restore_blob(&blob_hash).unwrap();
    assert_eq!(core.list_blobs().unwrap().len(), 1);

    // 清除碎片：shard 与 manifest 全部删除，墓碑记录保留
    core.delete_blob(&blob_hash).unwrap();
    core.purge_blob(&blob_hash).unwrap();
    assert_eq!(count_files(&dir.path().join("storages")), 0, "碎片应已清空");
    core.restore_blob(&blob_hash).unwrap();
    assert!(core.export_blob(&blob_hash, &dir.path().join("x.png")).is_err());
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
    // MK 封装架构下，设备需共享 mk.wrap 才能派生相同子密钥（模拟"凭口令加入"）
    std::fs::copy(
        dev_a.path().join("mk.wrap"),
        dev_b.path().join("mk.wrap"),
    )
    .unwrap();
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
fn rebalance_redistributes_shards_after_storage_change() {
    // M5 再平衡：导入后新增第三个 Storage，rebalance 把 shard 重排到
    // shard_index % 3 的策略位置，manifest 冗余回写，导出仍逐字节一致。
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("rebalance-pass").unwrap();

    let src = dir.path().join("photo.png");
    let blob = random_bytes(0x2BA1, 1_048_576 + 4567);
    std::fs::write(&src, &blob).unwrap();
    let outcome = core.import_file(&src).unwrap();
    let blob_hash = outcome.blob_hash.clone().unwrap();

    // 导入时 2 个 Storage（local-a/local-b）；追加第三个 → 策略位置改变
    core.add_storage_local("local-c", dir.path().join("storages").join("local-c"))
        .unwrap();
    let before = core.verify_blob(&blob_hash).unwrap();
    assert!(before.ok);

    let report = core.rebalance_all().unwrap();
    assert!(report.moved > 0, "expected shards to move: {report:?}");
    let after = core.verify_blob(&blob_hash).unwrap();
    assert!(after.ok, "all shards must be healthy after rebalance: {after:?}");

    // 每个位置与策略一致（无法从外部直接读 manifest——用导出验证数据完好）
    let out = dir.path().join("after_rebalance.png");
    core.export_blob(&blob_hash, &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), blob);

    // 再次 rebalance：幂等（无移动）
    let report2 = core.rebalance_all().unwrap();
    assert_eq!(report2.moved, 0, "second rebalance must be a no-op");
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

// ---------------------------------------------------------------------------
// §12.2 目录导入 E2E：五阶段向导（扫描→预览→幂等建相册→上传→报告）
// ---------------------------------------------------------------------------

fn write_file(dir: &std::path::Path, rel: &str, data: &[u8]) {
    let p = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}

/// §12.2：同一目录两次导入——第二次加新照片 → 增量上传 + 相册幂等复用（§8.3/§8.4）。
#[test]
fn directory_import_two_runs_incremental_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("dir-import").unwrap();

    let src_root = tempfile::tempdir().unwrap();
    write_file(src_root.path(), "beach.jpg", &random_bytes(0x11, 300_000));
    write_file(src_root.path(), "sub/mountain.png", &random_bytes(0x22, 200_000));
    write_file(src_root.path(), "sub/forest.jpg", &random_bytes(0x33, 100_000));

    // 第一次导入（3 文件，2 相册：根 + sub）
    let scan = core.import_scan_dir(src_root.path(), true).unwrap();
    assert_eq!(scan.files_scanned, 3);
    assert!(scan.dirs.contains(&"sub".to_string()));
    let rep1 = core
        .import_execute(&scan.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert_eq!(rep1.imported, 3);
    assert_eq!(rep1.albums_created, 2, "root + sub");
    assert!(!rep1.paused);

    // 第二次导入（无新增）：0 上传 + 全去重 + 相册按 import_source 复用
    let scan2 = core.import_scan_dir(src_root.path(), true).unwrap();
    let rep2 = core
        .import_execute(&scan2.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert_eq!(rep2.imported, 0, "no new files → no uploads");
    assert_eq!(rep2.skipped_duplicate, 3, "all deduped via staging/index");
    assert_eq!(rep2.albums_created, 0);
    assert_eq!(rep2.albums_reused, 2, "albums reused by import_source");

    // 第三次导入：目录里加 2 张新照片 → 只上传增量
    write_file(src_root.path(), "sub/sunset.jpg", &random_bytes(0x44, 150_000));
    write_file(src_root.path(), "city.jpg", &random_bytes(0x55, 250_000));
    let scan3 = core.import_scan_dir(src_root.path(), true).unwrap();
    assert_eq!(scan3.files_scanned, 5);
    let rep3 = core
        .import_execute(&scan3.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert_eq!(rep3.imported, 2, "only the 2 new photos upload");
    assert_eq!(rep3.skipped_duplicate, 3, "existing 3 deduped");
    assert_eq!(rep3.albums_created, 0, "albums reused (incremental)");
    assert_eq!(rep3.albums_reused, 2);

    // 全部 5 个 blob 在时间线里
    assert_eq!(core.list_blobs().unwrap().len(), 5);
}

/// §12.2：5000+ 文件目录——分批处理不 OOM，全部入库。
#[test]
fn directory_import_5000_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("bulk-import").unwrap();

    let src = tempfile::tempdir().unwrap();
    for i in 0..5000u32 {
        // 内容各异：避免 AV 将批量同内容文件隔离（真实照片本来互不相同）
        let mut payload = vec![0xA5u8; 128];
        payload[0..4].copy_from_slice(&i.to_be_bytes());
        write_file(src.path(), &format!("batch/{i:05}.bin"), &payload);
    }

    let scan = core.import_scan_dir(src.path(), false).unwrap();
    assert_eq!(scan.files_scanned, 5000);
    let rep = core
        .import_execute(&scan.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert_eq!(rep.imported, 5000, "failures: {:?}", rep.failed);
    assert!(!rep.paused);
    assert_eq!(core.list_blobs().unwrap().len(), 5000);
}

/// §12.2：导入中断网——存储失联触发连续失败 5 个暂停；恢复后续传完成。
#[test]
fn directory_import_resume_after_outage() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = HeartCore::new(dir.path());
    core.unlock("offline-import").unwrap();

    let src = tempfile::tempdir().unwrap();
    for i in 0..8u32 {
        // 种子取奇数且互不相同（xorshift 内部 seed|1 会吞掉偶数最低位）
        write_file(
            src.path(),
            &format!("photo/{i:02}.jpg"),
            &random_bytes(0x101 + u64::from(i) * 2, 64_000),
        );
    }
    let scan = core.import_scan_dir(src.path(), false).unwrap();

    // "断网"：移除默认 Storage，注册一个不可达的盘符（put 全部失败；
    // LocalDirStore 会自愈重建目录，改名模拟不了真实断网）。
    core.remove_storage("local-a").unwrap();
    core.remove_storage("local-b").unwrap();
    core.add_storage_local("offline", "Q:\\heartshard-offline-test").unwrap();

    let rep1 = core
        .import_execute(&scan.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert!(rep1.paused, "5 consecutive failures must pause: {}", rep1.failed.len());
    assert_eq!(rep1.imported, 0);
    assert_eq!(rep1.failed.len(), 5, "paused after 5 consecutive failures");

    // "恢复网络"：换回可用 Storage
    core.remove_storage("offline").unwrap();
    core.add_storage_local("local-c", dir.path().join("storages").join("local-c"))
        .unwrap();

    // 续传：staging 里 failed/pending 全部重试 → 完成
    let rep2 = core
        .import_execute(&scan.session_id, ImportOptions::default(), &AtomicBool::new(false), &|_| {})
        .unwrap();
    assert_eq!(rep2.imported, 8, "resume completes all 8: {:?}", rep2.failed);
    assert!(!rep2.paused);

    // 数据完好：全部可还原
    let blobs = core.list_blobs().unwrap();
    assert_eq!(blobs.len(), 8);
    for b in &blobs {
        let out = dir.path().join("restored").join(&b.file_name);
        core.export_blob(&b.blob_hash, &out).unwrap();
    }
}

