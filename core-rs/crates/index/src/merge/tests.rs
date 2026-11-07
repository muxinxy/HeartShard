use super::*;
use crate::models::{AlbumRecord, BlobRecord};

fn blob(hash: &str, taken_at: i64) -> BlobRecord {
    BlobRecord {
        hash: hash.to_string(),
        file_name: "photo.jpg".into(),
        size: 100,
        mime: "image/jpeg".into(),
        width: 0,
        height: 0,
        gps_lat: None,
        gps_lon: None,
        taken_at,
        chunk_size: 1_048_576,
        n: 4,
        m: 2,
        manifest_storage_key: format!("manifests/{hash}/manifest.bin"),
        created_at: 1,
        deleted_at: None,
        favorite: false,
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

#[test]
fn newer_lamport_wins() {
    let local = Versioned { lamport: 5, device_id: "dev-a".into(), record: album("a1", "旧名") };
    let remote = Versioned { lamport: 9, device_id: "dev-b".into(), record: album("a1", "新名") };
    assert_eq!(merge_album(&local, &remote), MergeOutcome::TookRemote);
    assert_eq!(merge_album(&remote, &local), MergeOutcome::KeptLocal);
}

#[test]
fn same_lamport_smaller_device_id_wins() {
    // 同钟：字典序较小 device_id 胜（"dev-a" < "dev-b"）
    let a = Versioned { lamport: 7, device_id: "dev-a".into(), record: album("a1", "来自A") };
    let b = Versioned { lamport: 7, device_id: "dev-b".into(), record: album("a1", "来自B") };
    assert_eq!(merge_album(&a, &b), MergeOutcome::KeptLocal); // a 胜
    assert_eq!(merge_album(&b, &a), MergeOutcome::TookRemote); // a 胜
}

#[test]
fn concurrent_album_rename_converges() {
    // 简报 §12.1：两设备并发写同一相册 → 两端各自应用对方后结果一致
    let a_ren = Versioned { lamport: 4, device_id: "dev-a".into(), record: album("a1", "A改的名字") };
    let b_ren = Versioned { lamport: 4, device_id: "dev-b".into(), record: album("a1", "B改的名字") };
    // 设备 A 本地 a_ren，收到 b_ren
    let at_a = merge_album(&a_ren, &b_ren);
    // 设备 B 本地 b_ren，收到 a_ren
    let at_b = merge_album(&b_ren, &a_ren);
    // A 保留本地（a 小），B 采用远端（a 小）——两边都收敛到 a_ren
    assert_eq!(at_a, MergeOutcome::KeptLocal);
    assert_eq!(at_b, MergeOutcome::TookRemote);
    let winner_at_a = if at_a == MergeOutcome::TookRemote { &b_ren } else { &a_ren };
    let winner_at_b = if at_b == MergeOutcome::TookRemote { &a_ren } else { &b_ren };
    assert_eq!(winner_at_a.record.name, winner_at_b.record.name);
}

#[test]
fn tombstone_beats_older_live_record() {
    let mut deleted = blob("ab12", 100);
    deleted.deleted_at = Some(999);
    let live = Versioned { lamport: 3, device_id: "dev-a".into(), record: blob("ab12", 100) };
    let tomb = Versioned { lamport: 4, device_id: "dev-b".into(), record: deleted };
    assert_eq!(merge_blob(&live, &tomb), MergeOutcome::TookRemote);
    // 但更旧的时间戳不会复活墓碑
    let stale_live = Versioned { lamport: 2, device_id: "dev-c".into(), record: blob("ab12", 100) };
    assert_eq!(merge_blob(&stale_live, &tomb), MergeOutcome::TookRemote);
}

#[test]
fn identical_version_equivalent() {
    let x = Versioned { lamport: 3, device_id: "dev-a".into(), record: blob("ab12", 5) };
    let y = Versioned { lamport: 3, device_id: "dev-a".into(), record: blob("ab12", 5) };
    // 同钟同设备：等价记录，保留本地即可（内容应一致）
    assert_eq!(merge_blob(&x, &y), MergeOutcome::KeptLocal);
    assert_eq!(x.record, y.record);
}

#[test]
fn lamport_folding() {
    assert_eq!(fold_lamport(5, 3), 5);
    assert_eq!(fold_lamport(3, 9), 9);
    assert_eq!(fold_lamport(0, 0), 0);
}
