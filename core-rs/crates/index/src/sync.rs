//! 同步引擎：全量状态发布 + 拉取合并（M2）。
//!
//! 流程（每次 `sync_now`）：
//! 1. **拉取**：对每个 Storage 列出 `index/` 下对象，跳过自己设备与已应用的
//!    `(device, lamport)`；解密 → 逐记录 LWW 合并入库；折叠远端时钟。
//! 2. **发布**：本地时钟 +1，打包本地全量状态为 [`IndexDelta`]（剥离
//!    local_only），加密后写入每个 Storage 的 `index/{me}/{lamport}.idx`。
//!    任一 Storage 成功即算发布成功（其余失败仅入报告）。
//!
//! 收敛性论证：全量发布 + per-record LWW（全序版本对）+ applied 幂等 ⇒
//! 任意顺序同步最终一致。对象数量随发布次数增长，GC 属 M5 运维任务。

use crypto::CryptoError;
use serde::Serialize;

use crate::delta::IndexDelta;
use crate::merge::fold_lamport;
use crate::sqlite::IndexDb;
use crate::store::BlobStore;
use crate::IndexError;

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub published_lamport: u64,
    pub pulled_objects: usize,
    pub applied_objects: usize,
    pub skipped_objects: Vec<String>,
    pub stores_ok: Vec<String>,
    pub stores_failed: Vec<String>,
}

fn parse_idx_key(key: &str) -> Option<(String, u64)> {
    let rest = key.strip_prefix("index/")?;
    let (device, file) = rest.split_once('/')?;
    if !file.ends_with(".idx") {
        return None;
    }
    let lamport: u64 = file.trim_end_matches(".idx").parse().ok()?;
    Some((device.to_string(), lamport))
}

/// 执行一次同步。`stores` 为全部启用的 Storage（≥1）。
pub fn sync_now(
    db: &IndexDb,
    stores: &[&dyn BlobStore],
    manifest_key: &[u8; 32],
    now: i64,
) -> Result<SyncReport, IndexError> {
    if stores.is_empty() {
        return Err(IndexError::Data("no storage configured".into()));
    }
    let me = db.device_id()?;
    let mut pulled_objects = 0usize;
    let mut applied_objects = 0usize;
    let mut skipped = Vec::new();
    let mut lamport = db.lamport()?;

    // ---- 拉取 + 合并 ----
    let mut seen_objects: Vec<(String, u64)> = Vec::new();
    for store in stores {
        let keys = store.list("index").map_err(IndexError::from)?;
        for key in keys {
            let Some((device, remote_lamport)) = parse_idx_key(&key) else {
                continue;
            };
            if device == me {
                continue; // 自己的旧发布对象，跳过
            }
            let dedup = (device.clone(), remote_lamport);
            if seen_objects.contains(&dedup) {
                continue; // 多 Storage 冗余副本只处理一次
            }
            seen_objects.push(dedup.clone());
            pulled_objects += 1;
            if db.applied_contains(&device, remote_lamport)? {
                continue;
            }
            let Some(bytes) = store.get(&key) else {
                skipped.push(key);
                continue;
            };
            let delta = match IndexDelta::decode(manifest_key, &bytes) {
                Ok(d) => d,
                Err(CryptoError::Aead) | Err(CryptoError::Base62) => {
                    // 损坏对象：跳过不阻断（防御性；正常情况下对象都是同钥加密）
                    skipped.push(key);
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            // 防御：delta 声明的 device_id 必须与路径一致
            if delta.device_id != device {
                skipped.push(key);
                continue;
            }
            for v in &delta.blobs {
                db.upsert_blob(v)?;
            }
            for v in &delta.albums {
                db.upsert_album(v)?;
            }
            for v in &delta.blob_albums {
                db.upsert_blob_album(v)?;
            }
            lamport = fold_lamport(lamport, delta.lamport);
            db.mark_applied(&device, remote_lamport, now)?;
            applied_objects += 1;
        }
    }

    // ---- 发布 ----
    // §7 delta：只发布 last_published 之后变化的记录（首次 last_published=0 → 全量）。
    // delta 为空时跳过发布（省对象；拉取端靠 applied 幂等）。
    let since = db.last_published()?;
    let mut delta = IndexDelta {
        device_id: me.clone(),
        lamport: lamport + 1,
        created_at: now,
        blobs: db.blobs_changed_since(since)?,
        albums: db.albums_changed_since(since)?,
        blob_albums: db.blob_albums_changed_since(since)?,
    };
    let new_lamport = lamport + 1;
    delta.strip_local_only();
    let delta_empty =
        delta.blobs.is_empty() && delta.albums.is_empty() && delta.blob_albums.is_empty();
    if delta_empty {
        // 无变化：不发布对象，时钟不推进
        db.set_last_published(lamport)?;
        return Ok(SyncReport {
            published_lamport: lamport,
            pulled_objects,
            applied_objects,
            skipped_objects: skipped,
            stores_ok: Vec::new(),
            stores_failed: Vec::new(),
        });
    }
    let key = delta.object_key()?;
    let iv = crypto::generate_iv()?;
    let payload = delta.encode(manifest_key, &iv)?;

    let mut stores_ok = Vec::new();
    let mut stores_failed = Vec::new();
    for store in stores {
        match store.put(&key, &payload) {
            Ok(()) => stores_ok.push(store.id().to_string()),
            Err(e) => stores_failed.push(format!("{}: {e}", store.id())),
        }
    }
    if stores_ok.is_empty() {
        return Err(IndexError::Data(format!(
            "publish failed on all storages: {}",
            stores_failed.join("; ")
        )));
    }

    db.set_lamport(new_lamport)?;
    db.set_last_published(new_lamport)?;
    Ok(SyncReport {
        published_lamport: new_lamport,
        pulled_objects,
        applied_objects,
        skipped_objects: skipped,
        stores_ok,
        stores_failed,
    })
}

#[cfg(test)]
mod tests;
