//! 同步引擎集成测试：append/mirror 策略下的 新增/修改/删除/移动 语义。

use heart_core::library::HeartCore;
use heartshard_syncer::{Config, DataConfig, SourceConfig};
use std::fs;

fn make_config(data_dir: &std::path::Path, src_root: &std::path::Path, strategy: &str, delete_mode: &str) -> Config {
    Config {
        data: DataConfig {
            dir: data_dir.to_string_lossy().to_string(),
            password: Some("test-pw".into()),
        },
        source: vec![SourceConfig {
            id: "test-src".into(),
            kind: "local".into(),
            root: Some(src_root.to_string_lossy().to_string()),
            strategy: strategy.into(),
            delete_mode: delete_mode.into(),
            album_root: None,
            s3: None,
            webdav: None,
            openlist: None,
        }],
    }
}

fn new_core(dir: &std::path::Path) -> HeartCore {
    let mut core = HeartCore::new(dir);
    core.unlock("test-pw").unwrap();
    core
}

fn write_file(dir: &std::path::Path, rel: &str, seed: u8, len: usize) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, vec![seed; len]).unwrap();
}

/// append：新增导入 + 相册映射；修改忽略；删除忽略（相册保留）。
#[test]
fn append_semantics() {
    let data = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let cfg = make_config(data.path(), src.path(), "append", "keep");
    let mut core = new_core(data.path());

    write_file(src.path(), "vacation/beach.jpg", 1, 50_000);
    write_file(src.path(), "work/doc.png", 2, 20_000);

    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.added, 2);

    // 相册树按目录建立
    let albums = core.list_albums().unwrap();
    let vacation = albums.iter().find(|a| a.name == "vacation").expect("vacation 相册");
    let work = albums.iter().find(|a| a.name == "work").expect("work 相册");
    assert_eq!(core.list_album_blobs(&vacation.id).unwrap().len(), 1);
    assert_eq!(core.list_album_blobs(&work.id).unwrap().len(), 1);

    // 修改内容 → append 忽略，仍是 1 张
    write_file(src.path(), "vacation/beach.jpg", 9, 60_000);
    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.added, 0);
    assert_eq!(core.list_album_blobs(&vacation.id).unwrap().len(), 1);

    // 删除源文件 → append 忽略，相册保留
    fs::remove_file(src.path().join("vacation/beach.jpg")).unwrap();
    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.removed, 0);
    assert_eq!(core.list_album_blobs(&vacation.id).unwrap().len(), 1);
}

/// mirror：删除跟进（trash）、移动只改相册关联不重传、修改进新版本旧版退役。
#[test]
fn mirror_semantics() {
    let data = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let cfg = make_config(data.path(), src.path(), "mirror", "trash");
    let mut core = new_core(data.path());

    write_file(src.path(), "2024/beach.jpg", 3, 40_000);
    heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();

    let albums = core.list_albums().unwrap();
    let a2024 = albums.iter().find(|a| a.name == "2024").unwrap();
    let blob_before = core.list_album_blobs(&a2024.id).unwrap().remove(0);

    // 移动（改名目录）→ 相册关联迁移，blob 复用
    fs::create_dir_all(src.path().join("2025")).unwrap();
    fs::rename(src.path().join("2024/beach.jpg"), src.path().join("2025/beach.jpg")).unwrap();
    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.moved, 1, "移动应只改关联");
    let a2025 = core.list_albums().unwrap().into_iter().find(|a| a.name == "2025").unwrap();
    let after_move = core.list_album_blobs(&a2025.id).unwrap();
    assert_eq!(after_move.len(), 1);
    assert_eq!(after_move[0].blob_hash, blob_before.blob_hash, "blob 不重传");
    // 旧相册已移除
    let a2024 = core.list_albums().unwrap().into_iter().find(|a| a.name == "2024").unwrap();
    assert!(core.list_album_blobs(&a2024.id).unwrap().is_empty());

    // 修改内容 → 新版本入库，旧版本进回收站
    write_file(src.path(), "2025/beach.jpg", 8, 55_000);
    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.added, 1);
    assert_eq!(core.list_album_blobs(&a2025.id).unwrap().len(), 1, "新版本替换");
    assert_eq!(core.list_trashed().unwrap().len(), 1, "旧版本进回收站");

    // 源删除 → mirror+trash：相册清空，进回收站
    fs::remove_file(src.path().join("2025/beach.jpg")).unwrap();
    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.removed, 1);
    assert!(core.list_album_blobs(&a2025.id).unwrap().is_empty());
}

/// 跨源去重：同一内容出现在两个子目录 → 第二个只加相册关联，不重复编码。
#[test]
fn cross_album_dedupe() {
    let data = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let cfg = make_config(data.path(), src.path(), "append", "keep");
    let mut core = new_core(data.path());

    let content = vec![7u8; 30_000];
    fs::create_dir_all(src.path().join("a")).unwrap();
    fs::create_dir_all(src.path().join("b")).unwrap();
    fs::write(src.path().join("a/dup.jpg"), &content).unwrap();
    fs::write(src.path().join("b/dup.jpg"), &content).unwrap();

    let r = heartshard_syncer::sync_source_for_test(&mut core, cfg.source.first().unwrap(), false).unwrap();
    assert_eq!(r.added, 2, "两个路径都计入");
    // 但 blob 只有一个（第二次是 SkippedDuplicate + 相册关联）
    assert_eq!(core.list_blobs().unwrap().len(), 1);
    let albums = core.list_albums().unwrap();
    let a = albums.iter().find(|x| x.name == "a").unwrap();
    let b = albums.iter().find(|x| x.name == "b").unwrap();
    assert_eq!(core.list_album_blobs(&a.id).unwrap().len(), 1);
    assert_eq!(core.list_album_blobs(&b.id).unwrap().len(), 1);
}
