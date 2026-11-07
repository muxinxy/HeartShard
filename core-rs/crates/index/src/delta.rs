//! 远端索引对象：`index/{deviceId}/{lamport}.idx`（简报 §7.2）。
//!
//! 编码：全量状态 JSON → AES-GCM 加密（MANIFEST_KEY + 随机 IV，bare 层）。
//! M2 采用全量发布（幂等、实现简单）；增量 delta 为后续优化，
//! 届时只需在此扩展 mutations 列表，合并端逻辑不变。

use serde::{Deserialize, Serialize};

use crate::models::{BlobAlbumRecord, AlbumRecord, BlobRecord, Versioned};
use crypto::{decrypt_bare, encrypt_bare, CryptoError};
use keypath::index_object_key;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDelta {
    pub device_id: String,
    /// 发布时刻该设备的 Lamport 时钟。
    pub lamport: u64,
    pub created_at: i64,
    /// `import_source`/`local_only` 字段在序列化前被剥离（§8.3，见 [`strip_album`]）。
    pub blobs: Vec<Versioned<BlobRecord>>,
    pub albums: Vec<Versioned<AlbumRecord>>,
    pub blob_albums: Vec<Versioned<BlobAlbumRecord>>,
}

impl IndexDelta {
    pub fn object_key(&self) -> Result<String, keypath::KeypathError> {
        index_object_key(&self.device_id, self.lamport)
    }

    /// 加密编码：JSON（字段序即结构体声明序）→ `iv‖ct‖tag`。
    pub fn encode(&self, manifest_key: &[u8; 32], iv: &[u8; 12]) -> Result<Vec<u8>, CryptoError> {
        let json = serde_json::to_vec(self).map_err(|_| CryptoError::Kdf("serialize delta".into()))?;
        encrypt_bare(manifest_key, iv, &json)
    }

    /// 解码并校验 device_id 一致性。
    pub fn decode(manifest_key: &[u8; 32], data: &[u8]) -> Result<Self, CryptoError> {
        let json = decrypt_bare(manifest_key, data)?;
        serde_json::from_slice(&json).map_err(|_| CryptoError::Kdf("deserialize delta".into()))
    }

    /// 发布前剥离 local_only 字段（§8.3：import_source 不同步）。
    /// 就地修改 albums 的 import_source 为 None（local_only 语义不变）。
    pub fn strip_local_only(&mut self) {
        for v in &mut self.albums {
            v.record.import_source = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AlbumRecord;

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    fn sample() -> IndexDelta {
        IndexDelta {
            device_id: "device01ab".into(),
            lamport: 42,
            created_at: 1700000000,
            blobs: vec![Versioned {
                lamport: 3,
                device_id: "device01ab".into(),
                record: crate::models::BlobRecord {
                    hash: "a".repeat(64),
                    file_name: "beach.png".into(),
                    size: 123,
                    mime: "image/png".into(),
                    width: 0,
                    height: 0,
                    gps_lat: None,
                    gps_lon: None,
                    taken_at: 1700000001,
                    chunk_size: 1048576,
                    n: 4,
                    m: 2,
                    manifest_storage_key: "manifests/x/manifest.bin".into(),
                    created_at: 1,
                    deleted_at: None,
                    favorite: false,
                },
            }],
            albums: vec![Versioned {
                lamport: 5,
                device_id: "device01ab".into(),
                record: AlbumRecord {
                    id: "alb-1".into(),
                    name: "旅行".into(),
                    parent_id: None,
                    sort_order: 0,
                    import_source: Some("D:/pics/travel".into()),
                    local_only: true,
        cover_hash: String::new(),
                    created_at: 1,
                    deleted_at: None,
                },
            }],
            blob_albums: vec![],
        }
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let delta = sample();
        let enc = delta.encode(&key(), &[7u8; 12]).unwrap();
        let dec = IndexDelta::decode(&key(), &enc).unwrap();
        assert_eq!(dec, delta);
    }

    #[test]
    fn wrong_key_fails() {
        let delta = sample();
        let enc = delta.encode(&key(), &[7u8; 12]).unwrap();
        let mut bad = key();
        bad[0] ^= 1;
        assert!(IndexDelta::decode(&bad, &enc).is_err());
    }

    #[test]
    fn object_key_format() {
        let delta = sample();
        assert_eq!(delta.object_key().unwrap(), "index/device01ab/42.idx");
    }

    #[test]
    fn strip_local_only_removes_import_source() {
        let mut delta = sample();
        delta.strip_local_only();
        assert!(delta.albums[0].record.import_source.is_none());
        // name 等同步字段不受影响
        assert_eq!(delta.albums[0].record.name, "旅行");
    }
}
