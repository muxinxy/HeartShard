use super::*;
use crate::models::{AlbumRecord, BlobRecord, Versioned};
use crate::sqlite::IndexDb;
use crate::store::LocalDirStore;

const KEY: [u8; 32] = [0x42; 32];

fn blob(hash: String, name_suffix: &str) -> BlobRecord {
    BlobRecord {
        hash,
        file_name: format!("photo-{name_suffix}.jpg"),
        size: 1,
        mime: format!("image/{name_suffix}"),
        width: 0,
        height: 0,
        gps_lat: None,
        gps_lon: None,
        taken_at: 100,
        chunk_size: 1048576,
        n: 4,
        m: 2,
        manifest_storage_key: format!("manifests/x/{name_suffix}.bin"),
        created_at: 1,
        deleted_at: None,
        favorite: false,
        is_motion: false,
        motion_video_hash: String::new(),
    }
}

fn album(id: &str, name: &str) -> AlbumRecord {
    AlbumRecord {
        id: id.to_string(),
        name: name.to_string(),
        parent_id: None,
        sort_order: 0,
        import_source: None,
        local_only: false,
        cover_hash: String::new(),
        created_at: 1,
        deleted_at: None,
    }
}

fn fresh_db(dir: &std::path::Path, device: &str) -> IndexDb {
    let db = IndexDb::open(dir.join(format!("{device}.index.db")), "index-key-test").unwrap();
    db.init_schema().unwrap();
    db.set_device_id(device).unwrap();
    db
}

/// 双设备共享两个 Storage（冗余），A 写入 → 双方同步 → 时间线一致。
#[test]
fn two_devices_converge_on_shared_storages() {
    let cloud_a = tempfile::tempdir().unwrap();
    let cloud_b = tempfile::tempdir().unwrap();
    let dev_a_dir = tempfile::tempdir().unwrap();
    let dev_b_dir = tempfile::tempdir().unwrap();

    let store_a1 = LocalDirStore::new(cloud_a.path(), "cloud-a");
    let store_a2 = LocalDirStore::new(cloud_b.path(), "cloud-b");
    // 同两朵"云"给设备 B（模拟同一用户的两个 Storage）
    let store_b1 = LocalDirStore::new(cloud_a.path(), "cloud-a");
    let store_b2 = LocalDirStore::new(cloud_b.path(), "cloud-b");

    let db_a = fresh_db(dev_a_dir.path(), "device-aaa");
    let db_b = fresh_db(dev_b_dir.path(), "device-bbb");

    // 设备 A 写入：1 blob + 1 相册 + 关联
    db_a.upsert_blob(&Versioned { lamport: 1, device_id: "device-aaa".into(), record: blob("a".repeat(64), "a") }).unwrap_or_else(|e| panic!("{e}"));
    db_a.upsert_album(&Versioned { lamport: 1, device_id: "device-aaa".into(), record: album("alb-1", "旅行") }).unwrap();
    db_a.upsert_blob_album(&Versioned {
        lamport: 1,
        device_id: "device-aaa".into(),
        record: crate::models::BlobAlbumRecord {
            blob_hash: "a".repeat(64),
            album_id: "alb-1".into(),
            added_at: 1,
            deleted_at: None,
        },
    }).unwrap();

    // 各自同步
    let rep_a = sync_now(&db_a, &[&store_a1, &store_a2], &KEY, 1000).unwrap();
    assert_eq!(rep_a.stores_ok.len(), 2, "publish to both storages");
    assert_eq!(rep_a.published_lamport, 1);

    let rep_b = sync_now(&db_b, &[&store_b1, &store_b2], &KEY, 1001).unwrap();
    assert_eq!(rep_b.applied_objects, 1, "B pulls A's delta");
    assert_eq!(rep_b.published_lamport, 2, "B folds A's clock then bumps");

    // B 的时间线与 A 一致
    assert_eq!(db_a.load_blobs().unwrap(), db_b.load_blobs().unwrap());
    assert_eq!(db_a.load_albums().unwrap(), db_b.load_albums().unwrap());
    assert_eq!(db_a.load_blob_albums().unwrap(), db_b.load_blob_albums().unwrap());

    // B 再同步一次：幂等（自己的发布 + 已应用的远端对象不会重复应用）
    let rep_b2 = sync_now(&db_b, &[&store_b1, &store_b2], &KEY, 1002).unwrap();
    assert_eq!(rep_b2.applied_objects, 0);

    // A 回拉 B 的 lamport-2 发布（B 只发布过一次）
    let rep_a2 = sync_now(&db_a, &[&store_a1, &store_a2], &KEY, 1003).unwrap();
    assert_eq!(rep_a2.applied_objects, 1);
    assert_eq!(db_a.load_blobs().unwrap(), db_b.load_blobs().unwrap());
}

#[test]
fn concurrent_same_album_writes_converge_by_device_id() {
    let cloud = tempfile::tempdir().unwrap();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();

    let store_a = LocalDirStore::new(cloud.path(), "cloud");
    let store_b = LocalDirStore::new(cloud.path(), "cloud");
    let db_a = fresh_db(dir_a.path(), "device-aaa");
    let db_b = fresh_db(dir_b.path(), "device-bbb");

    // 两设备先互相知道这个相册（lamport 1）
    db_a.upsert_album(&Versioned { lamport: 1, device_id: "device-aaa".into(), record: album("alb-1", "初始名") }).unwrap();
    sync_now(&db_a, &[&store_a], &KEY, 10).unwrap();
    sync_now(&db_b, &[&store_b], &KEY, 11).unwrap();

    // 并发：同钟 (lamport 2) 不同名
    db_a.set_lamport(1).unwrap();
    db_b.set_lamport(1).unwrap();
    db_a.upsert_album(&Versioned { lamport: 2, device_id: "device-aaa".into(), record: album("alb-1", "A改的名字") }).unwrap();
    db_b.upsert_album(&Versioned { lamport: 2, device_id: "device-bbb".into(), record: album("alb-1", "B改的名字") }).unwrap();

    sync_now(&db_a, &[&store_a], &KEY, 12).unwrap();
    sync_now(&db_b, &[&store_b], &KEY, 13).unwrap();
    // 各自拉取对方后必须收敛到同一 winner（字典序较小 device-aaa 胜）
    let albums_a = db_a.load_albums().unwrap();
    let albums_b = db_b.load_albums().unwrap();
    assert_eq!(albums_a, albums_b);
    assert_eq!(albums_a[0].record.name, "A改的名字");
    assert_eq!(albums_a[0].device_id, "device-aaa");
}

#[test]
fn corrupt_remote_object_skipped_not_fatal() {
    let cloud = tempfile::tempdir().unwrap();
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let store_a = LocalDirStore::new(cloud.path(), "cloud");
    let store_b = LocalDirStore::new(cloud.path(), "cloud");
    let db_a = fresh_db(dir_a.path(), "device-aaa");
    let db_b = fresh_db(dir_b.path(), "device-bbb");

    db_a.upsert_blob(&Versioned { lamport: 1, device_id: "device-aaa".into(), record: blob("b".repeat(64), "b") }).unwrap();
    sync_now(&db_a, &[&store_a], &KEY, 10).unwrap();

    // 塞入一个损坏的对象（同路径约定但内容是垃圾）
    store_a.put("index/device-ccc/5.idx", b"garbage-not-encrypted").unwrap();

    let rep = sync_now(&db_b, &[&store_b], &KEY, 11).unwrap();
    assert_eq!(rep.applied_objects, 1, "good object applied");
    assert_eq!(rep.skipped_objects.len(), 1, "corrupt object skipped");
    assert_eq!(db_b.load_blobs().unwrap().len(), 1);
}

#[test]
fn no_stores_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = fresh_db(dir.path(), "device-aaa");
    assert!(sync_now(&db, &[], &KEY, 0).is_err());
}
