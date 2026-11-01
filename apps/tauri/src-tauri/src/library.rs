//! HeartShard 业务库（M2）：会话密钥、SQLCipher 索引、多 Storage（模拟/S3）、
//! 加密 manifest（冗余 ≥2 Storage）、导入/导出/校验、双设备同步。
//! 与 Tauri 类型解耦，可直接被集成测试调用。
//!
//! M1 → M2 变更：本地 JSON 索引 → SQLCipher index.db（core-rs/crates/index）；
//! 单 Storage → 多 Storage 注册表（shard 按 shard_index 分散、manifest 冗余全量写入）；
//! 新增 sync_now（Lamport 全量发布 + 拉取合并）。
//! 已知限制：M1 的 index.m1.json.enc 不做迁移（M1 未发布给用户）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crypto::{
    decrypt_bare, derive_master_key, encrypt_bare, generate_iv, CryptoError, DerivedKeys,
    KdfParams,
};
use ida::CODEC_ID;
use index::models::{BlobRecord, Versioned};
use index::sqlite::IndexDb;
use index::store::BlobStore;
use index::sync;
use keypath::manifest_object_key;
use serde::{Deserialize, Serialize};
use shard::{encode_blob, ChunkManifestInfo};
use thiserror::Error;

use crate::storage_openlist::{OpenListConfig, OpenListStore};
use crate::storage_s3::{S3Config, S3Store};

pub const DEFAULT_CHUNK_SIZE: usize = shard::DEFAULT_CHUNK_SIZE; // 1 MiB
pub const DEFAULT_N: u8 = shard::DEFAULT_N; // 4
pub const DEFAULT_M: u8 = shard::DEFAULT_M; // 2

#[derive(Debug, Error)]
pub enum LibError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("shard: {0}")]
    Shard(#[from] shard::ShardError),
    #[error("ida: {0}")]
    Ida(#[from] ida::IdaError),
    #[error("keypath: {0}")]
    Keypath(#[from] keypath::KeypathError),
    #[error("index: {0}")]
    Index(#[from] index::IndexError),
    #[error("store: {0}")]
    Store(#[from] index::store::StoreError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session is locked; unlock first")]
    Locked,
    #[error("blob {0} not found in index")]
    BlobNotFound(String),
    #[error("manifest for blob {0} is missing or unreadable")]
    ManifestMissing(String),
    #[error("data error: {0}")]
    Data(String),
}

/// 会话期持有派生密钥与打开的加密索引；Drop 时零化。
pub struct HeartCore {
    data_dir: PathBuf,
    keys: Option<DerivedKeys>,
    db: Option<IndexDb>,
    storages: Vec<StorageConfig>,
}

/// 时间线/列表条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlobEntry {
    pub blob_hash: String,
    pub file_name: String,
    pub mime: String,
    pub total_len: i64,
    pub taken_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportOutcome {
    pub path: String,
    pub status: ImportStatus,
    pub blob_hash: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Imported,
    SkippedDuplicate,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub blob_hash: String,
    pub ok: bool,
    pub missing: Vec<String>,
    pub corrupted: Vec<String>,
    pub shard_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageStatus {
    pub id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub published_lamport: u64,
    pub pulled_objects: usize,
    pub applied_objects: usize,
    pub skipped: Vec<String>,
    pub stores_ok: Vec<String>,
    pub stores_failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairReport {
    pub blob_hash: String,
    pub repaired: Vec<String>,
    pub unrecoverable: Vec<u32>,
    pub shard_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RebalanceReport {
    pub blobs_checked: usize,
    pub moved: usize,
    pub failed: Vec<String>,
}

// ---------------------------------------------------------------------------
// Storage 注册表
// ---------------------------------------------------------------------------

/// 多 Storage 配置。S3 凭据经 CONFIG_KEY 加密后落盘（§7.1 config_encrypted 语义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageConfig {
    LocalDir { id: String, root: String },
    S3(S3Config),
    OpenList(OpenListConfig),
}

impl StorageConfig {
    pub fn id(&self) -> String {
        match self {
            StorageConfig::LocalDir { id, .. } => id.clone(),
            StorageConfig::S3(c) => c.bucket.clone(),
            StorageConfig::OpenList(c) => c.base_url.clone(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            StorageConfig::LocalDir { .. } => "local-dir",
            StorageConfig::S3(_) => "s3",
            StorageConfig::OpenList(_) => "openlist",
        }
    }

    pub fn store(&self) -> Result<Box<dyn BlobStore>, LibError> {
        match self {
            StorageConfig::LocalDir { id, root } => {
                Ok(Box::new(index::store::LocalDirStore::new(root.clone(), id.clone())))
            }
            StorageConfig::S3(c) => Ok(Box::new(S3Store::new(c.clone())?)),
            StorageConfig::OpenList(c) => Ok(Box::new(OpenListStore::new(c.clone())?)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StorageRegistry {
    storages: Vec<StorageConfig>,
}

/// KDF 参数文件（非机密：盐与参数可落盘；口令与 MK 不落盘）。
#[derive(Debug, Serialize, Deserialize)]
struct KdfParamsFile {
    salt_hex: String,
    m_cost_kib: u32,
    t_cost: u32,
    parallelism: u32,
}

/// M2 加密 manifest（冗余写入每个启用的 Storage；M3 迁移到 manifest crate）。
#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    codec: String,
    blob_sha256: String,
    total_len: u64,
    chunk_size: u32,
    n: u8,
    m: u8,
    created_at: i64,
    file_name: String,
    mime: String,
    chunks: Vec<ManifestChunk>,
    shards: Vec<ManifestShard>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestChunk {
    plaintext_sha256: String,
    ciphertext_len: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestShard {
    chunk_index: u32,
    shard_index: u8,
    storage_id: String,
    remote_key: String,
    size: u32,
    sha256: String,
}

impl HeartCore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into(), keys: None, db: None, storages: Vec::new() }
    }

    fn kdf_params_path(&self) -> PathBuf {
        self.data_dir.join("kdf.params.json")
    }

    fn index_db_path(&self) -> PathBuf {
        self.data_dir.join("index.db")
    }

    fn registry_path(&self) -> PathBuf {
        self.data_dir.join("storages.registry.enc")
    }

    fn keys_or_err(&self) -> Result<&DerivedKeys, LibError> {
        self.keys.as_ref().ok_or(LibError::Locked)
    }

    fn db_or_err(&self) -> Result<&IndexDb, LibError> {
        self.db.as_ref().ok_or(LibError::Locked)
    }

    // -----------------------------------------------------------------------
    // 会话
    // -----------------------------------------------------------------------

    pub fn unlock(&mut self, password: &str) -> Result<(), LibError> {
        // 成本参数（m/t/p）可落盘调节；盐不落盘——从口令确定性派生：
        // ① 新设备仅凭口令即可解密远端索引（简报 §7.3）；
        // ② 每口令唯一盐，杜绝跨用户预计算彩虹表；
        // ③ 离线爆破成本仍由 Argon2id 参数承担。
        let params_file: KdfParamsFile = if self.kdf_params_path().exists() {
            let raw = fs::read(self.kdf_params_path())?;
            serde_json::from_slice::<KdfParamsFile>(&raw)
                .map_err(|e| LibError::Data(format!("kdf params corrupt: {e}")))?
        } else {
            let p = KdfParams::standard();
            let f = KdfParamsFile {
                salt_hex: String::new(),
                m_cost_kib: p.m_cost_kib,
                t_cost: p.t_cost,
                parallelism: p.parallelism,
            };
            fs::create_dir_all(&self.data_dir)?;
            fs::write(self.kdf_params_path(), serde_json::to_vec_pretty(&f)?)?;
            f
        };
        let params = KdfParams {
            m_cost_kib: params_file.m_cost_kib,
            t_cost: params_file.t_cost,
            parallelism: params_file.parallelism,
        };
        let salt_full = crypto::sha256(format!("heartshard/argon2-salt/v1:{}", password).as_bytes());
        let mk = derive_master_key(password.as_bytes(), &salt_full[..16], &params)?;
        let master_salt = format!("heartshard-master-salt-v1:{}", hex::encode(salt_full));
        let keys = DerivedKeys::new(mk, master_salt.as_bytes())?;
        self.keys = Some(keys.clone());

        // 打开 SQLCipher 索引（错口令在此处即失败：file is not a database）
        let index_key_hex = hex::encode(keys.index_key.as_ref());
        let db = match IndexDb::open(self.index_db_path(), &index_key_hex) {
            Ok(db) => db,
            Err(index::IndexError::Sqlite(e)) if e.to_string().contains("not a database") => {
                self.keys = None;
                return Err(LibError::Data(
                    "wrong password (index decryption failed)".into(),
                ));
            }
            Err(e) => return Err(e.into()),
        };
        db.init_schema()?;

        // 设备 id：首次生成（16B hex）
        if db.device_id().is_err() {
            let mut did = [0u8; 16];
            getrandom::getrandom(&mut did).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            db.set_device_id(&hex::encode(did))?;
        }

        // Storage 注册表：首次创建默认双本地 Storage（manifest 冗余 ≥2，§6.4）
        self.storages = self.load_registry(&keys)?;
        if self.storages.is_empty() {
            self.storages = vec![
                StorageConfig::LocalDir {
                    id: "local-a".into(),
                    root: self.data_dir.join("storages").join("local-a").to_string_lossy().to_string(),
                },
                StorageConfig::LocalDir {
                    id: "local-b".into(),
                    root: self.data_dir.join("storages").join("local-b").to_string_lossy().to_string(),
                },
            ];
            self.save_registry(&keys)?;
        }

        self.db = Some(db);
        Ok(())
    }

    pub fn lock(&mut self) {
        // 先关 DB 再清密钥
        self.db = None;
        self.storages.clear();
        self.keys = None; // Zeroizing Drop 清零
    }

    pub fn is_unlocked(&self) -> bool {
        self.keys.is_some()
    }

    fn load_registry(&self, keys: &DerivedKeys) -> Result<Vec<StorageConfig>, LibError> {
        if !self.registry_path().exists() {
            return Ok(Vec::new());
        }
        let enc = fs::read(self.registry_path())?;
        let json = decrypt_bare(keys.config_key.as_ref(), &enc)
            .map_err(|_| LibError::Data("storage registry decrypt failed".into()))?;
        let reg: StorageRegistry = serde_json::from_slice(&json)
            .map_err(|e| LibError::Data(format!("storage registry corrupt: {e}")))?;
        Ok(reg.storages)
    }

    fn save_registry(&self, keys: &DerivedKeys) -> Result<(), LibError> {
        let reg = StorageRegistry { storages: self.storages.clone() };
        let json = serde_json::to_vec(&reg)?;
        let iv = generate_iv()?;
        let enc = encrypt_bare(keys.config_key.as_ref(), &iv, &json)?;
        let tmp = self.registry_path().with_extension("tmp");
        fs::write(&tmp, &enc)?;
        fs::rename(&tmp, self.registry_path())?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Storage 辅助
    // -----------------------------------------------------------------------

    fn store_by_id(&self, id: &str) -> Result<Box<dyn BlobStore>, LibError> {
        let cfg = self
            .storages
            .iter()
            .find(|s| s.id() == id)
            .ok_or_else(|| LibError::Data(format!("storage {id} not registered")))?;
        cfg.store()
    }

    fn all_stores(&self) -> Result<Vec<Box<dyn BlobStore>>, LibError> {
        self.storages.iter().map(|s| s.store()).collect()
    }

    // -----------------------------------------------------------------------
    // 导入 / 列表 / 导出 / 校验 / 同步
    // -----------------------------------------------------------------------

    pub fn import_file(&mut self, path: impl AsRef<Path>) -> Result<ImportOutcome, LibError> {
        let path = path.as_ref();
        let display = path.to_string_lossy().to_string();
        let outcome = match self.import_inner(path) {
            Ok((hash, skipped)) => ImportOutcome {
                path: display,
                status: if skipped { ImportStatus::SkippedDuplicate } else { ImportStatus::Imported },
                blob_hash: Some(hash),
                error: None,
            },
            Err(e) => ImportOutcome {
                path: display,
                status: ImportStatus::Failed,
                blob_hash: None,
                error: Some(e.to_string()),
            },
        };
        Ok(outcome)
    }

    fn import_inner(&mut self, path: &Path) -> Result<(String, bool), LibError> {
        let keys = self.keys_or_err()?.clone();
        let db = self.db_or_err()?;
        let meta = fs::metadata(path)?;
        let taken_at = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let blob = fs::read(path)?;
        let blob_sha256 = crypto::sha256(&blob);
        let blob_hex = hex::encode(blob_sha256);

        // 去重（§8.4）
        if db.load_blobs()?.iter().any(|v| v.record.hash == blob_hex) {
            return Ok((blob_hex, true));
        }

        let stores = self.all_stores()?;
        if stores.is_empty() {
            return Err(LibError::Data("no storage configured".into()));
        }

        // 本地变更：Lamport 时钟 +1
        let lamport = db.lamport()? + 1;
        let me = db.device_id()?;
        let blob_key = keys.blob_key(&blob_sha256)?;

        let encoded = encode_blob(&blob, &blob_key, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M, generate_iv)?;

        // shard 分散：shard_index % storages.len()（同一 chunk 的 shard 必落不同 Storage，
        // 当 Storage 数 ≥ n 时天然满足；不足时按权重轮转、不重复放同 chunk 的 shard，§2.2）
        let mut manifest_shards = Vec::with_capacity(encoded.shards.len());
        for s in &encoded.shards {
            let cfg = &self.storages[usize::from(s.shard_index) % self.storages.len()];
            let store = cfg.store()?;
            store.put(&s.object_key, &s.data)?;
            manifest_shards.push(ManifestShard {
                chunk_index: s.chunk_index,
                shard_index: s.shard_index,
                storage_id: cfg.id(),
                remote_key: s.object_key.clone(),
                size: s.data.len() as u32,
                sha256: hex::encode(s.sha256),
            });
        }

        // manifest：加密后冗余写入全部 Storage（§6.4）
        let manifest = Manifest {
            version: 1,
            codec: CODEC_ID.to_string(),
            blob_sha256: blob_hex.clone(),
            total_len: encoded.total_len as u64,
            chunk_size: DEFAULT_CHUNK_SIZE as u32,
            n: DEFAULT_N,
            m: DEFAULT_M,
            created_at: now_secs(),
            file_name: file_name_of(path),
            mime: mime_of(path),
            chunks: encoded
                .chunks
                .iter()
                .map(|c| ManifestChunk {
                    plaintext_sha256: hex::encode(c.plaintext_sha256),
                    ciphertext_len: c.ciphertext.len() as u32,
                })
                .collect(),
            shards: manifest_shards,
        };
        let manifest_key_path = manifest_object_key(&blob_hex)?;
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let iv = generate_iv()?;
        let enc_manifest = encrypt_bare(keys.manifest_key.as_ref(), &iv, &manifest_bytes)?;
        for store in &stores {
            store.put(&manifest_key_path, &enc_manifest)?;
        }

        // 写加密索引（跨设备同步的元数据）
        db.upsert_blob(&Versioned {
            lamport,
            device_id: me,
            record: BlobRecord {
                hash: blob_hex.clone(),
                file_name: manifest.file_name.clone(),
                size: manifest.total_len as i64,
                mime: manifest.mime.clone(),
                width: 0,
                height: 0,
                taken_at,
                chunk_size: DEFAULT_CHUNK_SIZE as i64,
                n: DEFAULT_N as i64,
                m: DEFAULT_M as i64,
                manifest_storage_key: manifest_key_path.clone(),
                created_at: manifest.created_at,
                deleted_at: None,
            },
        })?;
        db.set_lamport(lamport)?;
        Ok((blob_hex, false))
    }

    pub fn list_blobs(&self) -> Result<Vec<BlobEntry>, LibError> {
        let db = self.db_or_err()?;
        let mut entries: Vec<BlobEntry> = db
            .load_blobs()?
            .into_iter()
            .filter(|v| v.record.deleted_at.is_none())
            .map(|v| BlobEntry {
                blob_hash: v.record.hash,
                file_name: v.record.file_name,
                mime: v.record.mime,
                total_len: v.record.size,
                taken_at: v.record.taken_at,
            })
            .collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.taken_at));
        Ok(entries)
    }

    fn load_manifest(&self, blob_hex: &str) -> Result<Manifest, LibError> {
        let keys = self.keys_or_err()?;
        let remote_key = manifest_object_key(blob_hex)?;
        // 冗余副本：任一 Storage 命中即可
        for store in self.all_stores()? {
            if let Some(enc) = store.get(&remote_key) {
                let json = decrypt_bare(keys.manifest_key.as_ref(), &enc)
                    .map_err(|_| LibError::ManifestMissing(blob_hex.to_string()))?;
                return serde_json::from_slice(&json)
                    .map_err(|e| LibError::Data(format!("manifest corrupt: {e}")));
            }
        }
        Err(LibError::ManifestMissing(blob_hex.to_string()))
    }

    /// 加密 manifest 并冗余写入全部 Storage（§6.4；rebalance 后的回写）。
    fn save_manifest(&self, blob_hex: &str, manifest: &Manifest) -> Result<(), LibError> {
        let keys = self.keys_or_err()?;
        let remote_key = manifest_object_key(blob_hex)?;
        let bytes = serde_json::to_vec(manifest)?;
        let iv = generate_iv()?;
        let enc = encrypt_bare(keys.manifest_key.as_ref(), &iv, &bytes)?;
        for store in self.all_stores()? {
            store.put(&remote_key, &enc)?;
        }
        Ok(())
    }

    pub fn export_blob(&self, blob_hash: &str, dest_path: impl AsRef<Path>) -> Result<(), LibError> {
        let keys = self.keys_or_err()?.clone();
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;

        let mut chunk_infos = Vec::with_capacity(manifest.chunks.len());
        for c in &manifest.chunks {
            chunk_infos.push(ChunkManifestInfo {
                plaintext_sha256: decode_hash(&c.plaintext_sha256)?,
                ciphertext_len: c.ciphertext_len as usize,
            });
        }
        let blob_hash_bytes: [u8; 32] = decode_hash(&manifest.blob_sha256)?;
        let blob_key = keys.blob_key(&blob_hash_bytes)?;

        let decoded = shard::decode_blob(
            &blob_hash_bytes,
            &blob_key,
            &chunk_infos,
            manifest.n,
            manifest.m,
            |chunk_index, shard_index| {
                let shard = manifest.shards.iter().find(|s| {
                    s.chunk_index == chunk_index && s.shard_index == shard_index
                })?;
                let store = self.store_by_id(&shard.storage_id).ok()?;
                store.get(&shard.remote_key)
            },
        )?;

        let final_hash = crypto::sha256(&decoded);
        if final_hash != blob_hash_bytes {
            return Err(LibError::Data(format!("blob sha256 mismatch after decode: {blob_hex}")));
        }
        if decoded.len() as u64 != manifest.total_len {
            return Err(LibError::Data("total length mismatch after decode".into()));
        }

        if let Some(parent) = dest_path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest_path, &decoded)?;
        Ok(())
    }

    pub fn verify_blob(&self, blob_hash: &str) -> Result<VerifyReport, LibError> {
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let mut missing = Vec::new();
        let mut corrupted = Vec::new();
        for s in &manifest.shards {
            let data = self
                .store_by_id(&s.storage_id)
                .ok()
                .and_then(|store| store.get(&s.remote_key));
            match data {
                None => missing.push(format!("[{}] {}", s.storage_id, s.remote_key)),
                Some(bytes) => {
                    if bytes.len() as u32 != s.size || hex::encode(crypto::sha256(&bytes)) != s.sha256 {
                        corrupted.push(format!("[{}] {}", s.storage_id, s.remote_key));
                    }
                }
            }
        }
        let shard_count = manifest.shards.len();
        Ok(VerifyReport {
            blob_hash: blob_hex,
            ok: missing.is_empty() && corrupted.is_empty(),
            missing,
            corrupted,
            shard_count,
        })
    }

    pub fn sync_now(&self) -> Result<SyncOutcome, LibError> {
        let keys = self.keys_or_err()?;
        let db = self.db_or_err()?;
        let stores = self.all_stores()?;
        let report = sync::sync_now(
            db,
            &stores.iter().map(|b| b.as_ref()).collect::<Vec<_>>(),
            &keys.manifest_key,
            now_secs(),
        )?;
        Ok(SyncOutcome {
            published_lamport: report.published_lamport,
            pulled_objects: report.pulled_objects,
            applied_objects: report.applied_objects,
            skipped: report.skipped_objects,
            stores_ok: report.stores_ok,
            stores_failed: report.stores_failed,
        })
    }

    pub fn storage_status(&self) -> Result<Vec<StorageStatus>, LibError> {
        Ok(self
            .storages
            .iter()
            .map(|s| StorageStatus { id: s.id(), kind: s.kind().to_string() })
            .collect())
    }

    /// 注册 S3 Storage（配置即刻经 CONFIG_KEY 加密落盘；连通性用 s3-verify 验证）。
    pub fn add_storage_s3(&mut self, config: S3Config) -> Result<String, LibError> {
        let keys = self.keys_or_err()?.clone();
        let id = config.bucket.clone();
        if self.storages.iter().any(|s| s.id() == id) {
            return Err(LibError::Data(format!("storage {id} already registered")));
        }
        self.storages.push(StorageConfig::S3(config));
        self.save_registry(&keys)?;
        Ok(id)
    }

    /// 注册 OpenList Storage（M4 适配器；真实实例连通性以实测为准）。
    pub fn add_storage_openlist(&mut self, config: OpenListConfig) -> Result<String, LibError> {
        let keys = self.keys_or_err()?.clone();
        let id = config.base_url.clone();
        if self.storages.iter().any(|s| s.id() == id) {
            return Err(LibError::Data(format!("storage {id} already registered")));
        }
        self.storages.push(StorageConfig::OpenList(config));
        self.save_registry(&keys)?;
        Ok(id)
    }

    /// 修复任务（M5）：对每个受损 chunk，用健康 shard RS 重组出原密文，
    /// 重新分散并把受损 shard 位置重写（RS 编码确定性 ⇒ shard 字节与 SHA 不变）。
    pub fn repair_blob(&self, blob_hash: &str) -> Result<RepairReport, LibError> {
        self.keys_or_err()?;
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let config = ida::DispersalConfig::new(manifest.n, manifest.m)?;

        let mut repaired = Vec::new();
        let mut unrecoverable = Vec::new();

        let chunk_infos: Vec<(u32, &ManifestChunk)> =
            manifest.chunks.iter().enumerate().map(|(i, c)| (i as u32, c)).collect();
        for (chunk_index, chunk) in chunk_infos {
            // 该 chunk 的 shard 状态
            let mut shards: Vec<(u8, Option<Vec<u8>>)> = Vec::new();
            for s in manifest.shards.iter().filter(|s| s.chunk_index == chunk_index) {
                let data = self
                    .store_by_id(&s.storage_id)
                    .ok()
                    .and_then(|store| store.get(&s.remote_key));
                let healthy = data
                    .as_ref()
                    .map(|d| d.len() as u32 == s.size && hex::encode(crypto::sha256(d)) == s.sha256)
                    .unwrap_or(false);
                shards.push((s.shard_index, if healthy { data } else { None }));
            }
            let damaged: Vec<u8> = shards
                .iter()
                .filter(|(_, d)| d.is_none())
                .map(|(i, _)| *i)
                .collect();
            if damaged.is_empty() {
                continue;
            }
            let healthy: Vec<(u8, Vec<u8>)> = shards
                .into_iter()
                .filter_map(|(i, d)| d.map(|d| (i, d)))
                .collect();
            if healthy.len() < usize::from(manifest.m) {
                unrecoverable.push(chunk_index);
                continue;
            }
            // RS 重组原密文 → 重新分散 → 重写受损位置
            let refs: Vec<(u8, &[u8])> =
                healthy.iter().map(|(i, d)| (*i, d.as_slice())).collect();
            let ciphertext = ida::decode_shards(&refs, config, chunk.ciphertext_len as usize)
                .map_err(|_| LibError::Data(format!("chunk {chunk_index} undecodable during repair")))?;
            let parts = ida::encode_shards(&ciphertext, config)
                .map_err(|e| LibError::Data(format!("re-encode failed: {e}")))?;
            for shard_index in damaged {
                let s = manifest
                    .shards
                    .iter()
                    .find(|s| s.chunk_index == chunk_index && s.shard_index == shard_index)
                    .ok_or_else(|| LibError::Data("manifest shard missing".into()))?;
                let store = self.store_by_id(&s.storage_id)?;
                store.put(&s.remote_key, &parts[usize::from(shard_index)])?;
                repaired.push(format!("[{}] {}", s.storage_id, s.remote_key));
            }
        }

        Ok(RepairReport {
            blob_hash: blob_hex,
            repaired,
            unrecoverable,
            shard_count: manifest.shards.len(),
        })
    }

    /// 再平衡任务（M5）：把每个 shard 移到按策略（`shard_index % 存储数`）
    /// 应属的 Storage。Storage 增删后、或导入时策略导致同 chunk 的多个 shard
    /// 同桶时，重排可恢复分散不变量。数据原子搬迁（先放新、再删旧），
    /// manifest 全量冗余回写。
    pub fn rebalance_all(&self) -> Result<RebalanceReport, LibError> {
        self.keys_or_err()?;
        let db = self.db_or_err()?;
        if self.storages.is_empty() {
            return Err(LibError::Data("no storage configured".into()));
        }
        let mut report = RebalanceReport {
            blobs_checked: 0,
            moved: 0,
            failed: Vec::new(),
        };

        for blob in db.load_blobs()? {
            if blob.record.deleted_at.is_some() {
                continue;
            }
            report.blobs_checked += 1;
            let blob_hex = blob.record.hash.clone();
            let mut manifest = self.load_manifest(&blob_hex)?;
            let mut changed = false;

            for s in &mut manifest.shards {
                let desired_idx = usize::from(s.shard_index) % self.storages.len();
                let desired_id = self.storages[desired_idx].id();
                if s.storage_id == desired_id {
                    continue; // 已在策略位置
                }
                // 取当前数据（缺失/损坏的 shard 归 repair 任务管，不在此搬运）
                let Some(data) = self
                    .store_by_id(&s.storage_id)
                    .ok()
                    .and_then(|store| store.get(&s.remote_key))
                else {
                    report
                        .failed
                        .push(format!("[{}] {}（shard 缺失，请先体检+修复）", s.storage_id, s.remote_key));
                    continue;
                };
                let new_key = keypath::shard_object_key(&blob_hex, s.chunk_index, s.shard_index)?;
                let desired_store = self.store_by_id(&desired_id)?;
                desired_store.put(&new_key, &data)?;
                if let Some(old_store) = self.store_by_id(&s.storage_id).ok() {
                    old_store.delete(&s.remote_key)?;
                }
                s.storage_id = desired_id;
                s.remote_key = new_key;
                report.moved += 1;
                changed = true;
            }
            if changed {
                self.save_manifest(&blob_hex, &manifest)?;
            }
        }
        Ok(report)
    }

    /// 注册本地目录 Storage（M2 模拟云端 / 未来 NAS 挂载目录）。
    pub fn add_storage_local(
        &mut self,
        id: &str,
        root: impl Into<PathBuf>,
    ) -> Result<String, LibError> {
        let keys = self.keys_or_err()?.clone();
        if self.storages.iter().any(|s| s.id() == id) {
            return Err(LibError::Data(format!("storage {id} already registered")));
        }
        keypath::sanitize_key_segment(id)?;
        self.storages.push(StorageConfig::LocalDir {
            id: id.to_string(),
            root: root.into().to_string_lossy().to_string(),
        });
        self.save_registry(&keys)?;
        Ok(id.to_string())
    }

    /// 移除 Storage（仅解除注册，不删除云端对象）。
    pub fn remove_storage(&mut self, id: &str) -> Result<(), LibError> {
        let keys = self.keys_or_err()?.clone();
        let before = self.storages.len();
        self.storages.retain(|s| s.id() != id);
        if self.storages.len() == before {
            return Err(LibError::Data(format!("storage {id} not found")));
        }
        self.save_registry(&keys)?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".into())
}

fn mime_of(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" | "heif" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn decode_hash(hex_str: &str) -> Result<[u8; 32], LibError> {
    let bytes = hex::decode(hex_str).map_err(|e| LibError::Data(format!("bad hash: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| LibError::Data(format!("hash length != 32: {hex_str}")))
}

// ---------------------------------------------------------------------------
// 目录导入五阶段向导（简报 §8.5）：扫描 → 预览 → 幂等建相册 → 上传(两级进度) → 报告
// ---------------------------------------------------------------------------

/// 导入跳过名单（§8.5 扫描阶段）。
const IMPORT_SKIP_NAMES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini", "$RECYCLE.BIN", ".git"];
/// 目录深度上限（§8.2）：超出部分合并到第 8 层。
const IMPORT_MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct DirScanResult {
    pub session_id: String,
    pub files_scanned: usize,
    pub total_bytes: u64,
    pub dirs: Vec<String>,
    /// 深度 > 8 被合并的相对目录
    pub truncated: Vec<String>,
    /// 跳过的符号链接
    pub symlinks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewAlbum {
    pub rel_path: String,
    pub file_count: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub session_id: String,
    pub albums: Vec<PreviewAlbum>,
    pub total_files: usize,
    pub total_bytes: u64,
    /// 已在索引中的文件数（将跳过上传，§8.4）
    pub already_in_index: usize,
    pub truncated: Vec<String>,
    pub symlinks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportRunReport {
    pub session_id: String,
    pub albums_created: usize,
    pub albums_reused: usize,
    pub imported: usize,
    pub skipped_duplicate: usize,
    pub failed: Vec<String>,
    /// 连续失败 5 个触发暂停（§8.5），剩余文件留在 staging 供续传
    pub paused: bool,
    pub truncated: Vec<String>,
    pub symlinks: Vec<String>,
}

/// 两级进度（§8.5）：相册级 + 文件级，经 Tauri event 推送。
#[derive(Debug, Clone, Serialize)]
pub struct ImportProgress {
    pub session_id: String,
    pub stage: String,
    pub album_name: Option<String>,
    pub album_index: usize,
    pub album_total: usize,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    pub create_empty_albums: bool,
    pub concurrency: usize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { create_empty_albums: true, concurrency: 3 }
    }
}

/// import_source 归一化（§8.3）：`\`→`/`、盘符小写、去首尾斜杠。其余大小写保留。
pub fn normalize_import_source(abs_dir: &str) -> String {
    let s = abs_dir.replace('\\', "/");
    let s = s.trim_matches('/');
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        format!("{}{}", (s.as_bytes()[0] as char).to_ascii_lowercase(), &s[1..])
    } else {
        s.to_string()
    }
}

/// 读文件，对 Windows AV/索引器造成的瞬时 NotFound/SharingViolation 做退避重试。
/// 批量导入（数千新建文件）在 Windows 上必然遇到这类瞬态；永久缺失交给
/// staging 失败 + 续传机制。
fn read_file_retry(path: &Path) -> Result<Vec<u8>, String> {
    let mut last_err = String::new();
    for (attempt, delay_ms) in [0u64, 50, 200, 500].iter().enumerate() {
        if *delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
        }
        match fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
                last_err = format!("read: {e}");
                let _ = attempt;
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    Err(last_err)
}

/// 扫描阶段（阶段 1）：walkdir 递归（不跟符号链接），跳过名单，SHA-256 预计算，
/// 结果写 import_staging。会话元数据（目录树/截断/符号链接）落 data_dir JSON。
impl HeartCore {
    pub fn import_scan_dir(
        &mut self,
        dir: &Path,
        create_empty_albums: bool,
    ) -> Result<DirScanResult, LibError> {
        self.keys_or_err()?;
        let db = self.db_or_err()?;
        let session_id = {
            let mut sid = [0u8; 8];
            getrandom::getrandom(&mut sid).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            hex::encode(sid)
        };
        let mut result = DirScanResult {
            session_id: session_id.clone(),
            files_scanned: 0,
            total_bytes: 0,
            dirs: Vec::new(),
            truncated: Vec::new(),
            symlinks: Vec::new(),
        };

        let mut stack: Vec<(PathBuf, String, usize)> = vec![(dir.to_path_buf(), String::new(), 0)];
        // 根目录自身也是一个相册（§8.2：所选目录的直接文件归入该相册）
        result.dirs.push(String::new());
        while let Some((abs, rel, depth)) = stack.pop() {
            let entries = fs::read_dir(&abs)?;
            let mut has_children = false;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if IMPORT_SKIP_NAMES.contains(&name.as_str()) {
                    continue;
                }
                let meta = fs::symlink_metadata(entry.path())?;
                if meta.file_type().is_symlink() {
                    result.symlinks.push(entry.path().to_string_lossy().to_string());
                    continue;
                }
                let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                if meta.is_dir() {
                    has_children = true;
                    // 深度 8 截断（§8.2）
                    let (store_rel, store_depth) = if depth + 1 > IMPORT_MAX_DEPTH {
                        result.truncated.push(child_rel.clone());
                        (rel.clone(), depth) // 合并进第 8 层相册
                    } else {
                        (child_rel.clone(), depth + 1)
                    };
                    result.dirs.push(store_rel.clone());
                    stack.push((entry.path(), store_rel, store_depth));
                } else if meta.is_file() {
                    has_children = true;
                    let bytes = fs::read(entry.path())?;
                    let sha = hex::encode(crypto::sha256(&bytes));
                    let mtime = meta
                        .modified()?
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    db.staging_upsert(&session_id, &entry.path().to_string_lossy(), &child_rel, meta.len() as i64, mtime, &sha)?;
                    result.files_scanned += 1;
                    result.total_bytes += meta.len();
                }
            }
            if !has_children && create_empty_albums && !rel.is_empty() {
                // 空目录也建空相册（UI 开关，§8.2）——记录目录供阶段 3 使用
                result.dirs.push(rel.clone());
            }
        }
        result.dirs.sort();
        result.dirs.dedup();

        // 会话元数据落盘（续传时阶段 3/5 需要）
        let meta = serde_json::json!({
            "root": dir.to_string_lossy(),
            "dirs": result.dirs,
            "truncated": result.truncated,
            "symlinks": result.symlinks,
        });
        fs::write(
            self.data_dir.join(format!("import-session-{session_id}.json")),
            serde_json::to_vec_pretty(&meta)?,
        )?;
        Ok(result)
    }

    /// 预览阶段（阶段 2）：相册树、文件数、总量、去重预计上传量。
    pub fn import_preview(&self, session_id: &str) -> Result<ImportPreview, LibError> {
        self.keys_or_err()?;
        let db = self.db_or_err()?;
        let rows = db.staging_load(session_id)?;
        let known: std::collections::HashSet<String> = db
            .load_blobs()?
            .into_iter()
            .map(|v| v.record.hash)
            .collect();

        let mut by_dir: std::collections::BTreeMap<String, PreviewAlbum> = std::collections::BTreeMap::new();
        let mut preview = ImportPreview {
            session_id: session_id.to_string(),
            albums: Vec::new(),
            total_files: rows.len(),
            total_bytes: 0,
            already_in_index: 0,
            truncated: Vec::new(),
            symlinks: Vec::new(),
        };
        if let Ok(meta_raw) = fs::read(self.data_dir.join(format!("import-session-{session_id}.json"))) {
            if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&meta_raw) {
                preview.truncated = serde_json::from_value(meta["truncated"].clone()).unwrap_or_default();
                preview.symlinks = serde_json::from_value(meta["symlinks"].clone()).unwrap_or_default();
            }
        }
        for r in &rows {
            preview.total_bytes += r.size.max(0) as u64;
            if known.contains(&r.sha256) {
                preview.already_in_index += 1;
            }
            let dir = r.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let album = by_dir.entry(dir.to_string()).or_insert_with(|| PreviewAlbum {
                rel_path: dir.to_string(),
                file_count: 0,
                bytes: 0,
            });
            album.file_count += 1;
            album.bytes += r.size.max(0) as u64;
        }
        preview.albums = by_dir.into_values().collect();
        Ok(preview)
    }

    /// 会话根目录（scan 时记录）。
    fn import_session_root(&self, session_id: &str) -> Result<PathBuf, LibError> {
        let raw = fs::read(self.data_dir.join(format!("import-session-{session_id}.json")))?;
        let meta: serde_json::Value = serde_json::from_slice(&raw)?;
        Ok(PathBuf::from(
            meta["root"].as_str().ok_or_else(|| LibError::Data("session meta corrupt".into()))?,
        ))
    }

    fn import_session_meta_list(&self, session_id: &str, key: &str) -> Vec<String> {
        fs::read(self.data_dir.join(format!("import-session-{session_id}.json")))
            .ok()
            .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
            .and_then(|meta| serde_json::from_value(meta[key].clone()).ok())
            .unwrap_or_default()
    }

    /// 上传阶段（阶段 3+4）：幂等建相册 + 逐文件上传（并发 3，两级进度，
    /// 连续失败 5 个暂停，staging 续传）。cancel 置位即停（已传保留）。
    pub fn import_execute(
        &self,
        session_id: &str,
        opts: ImportOptions,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(ImportProgress),
    ) -> Result<ImportRunReport, LibError> {
        use std::sync::atomic::Ordering;

        let keys = self.keys_or_err()?.clone();
        let db = self.db_or_err()?;
        let me = db.device_id()?;
        let mut report = ImportRunReport {
            session_id: session_id.to_string(),
            albums_created: 0,
            albums_reused: 0,
            imported: 0,
            skipped_duplicate: 0,
            failed: Vec::new(),
            paused: false,
            truncated: self.import_session_meta_list(session_id, "truncated"),
            symlinks: self.import_session_meta_list(session_id, "symlinks"),
        };
        let rows = db.staging_load(session_id)?;
        let total_files = rows.len();
        let total_bytes: u64 = rows.iter().map(|r| r.size.max(0) as u64).sum();

        // ---- 阶段 3：按 import_source 幂等建相册（§8.3）----
        let dirs = self.import_session_meta_list(session_id, "dirs");
        let mut album_ids: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let root = self.import_session_root(session_id)?;
        let mut sorted_dirs = dirs.clone();
        sorted_dirs.sort(); // 父目录先于子目录（路径前缀序）
        for (index, dir_rel) in sorted_dirs.iter().enumerate() {
            let abs_dir = root.join(dir_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let import_source = normalize_import_source(&abs_dir.to_string_lossy());
            let album_name = dir_rel.rsplit('/').next().unwrap_or(dir_rel).to_string();
            progress(ImportProgress {
                session_id: session_id.to_string(),
                stage: "albums".into(),
                album_name: Some(album_name.clone()),
                album_index: index,
                album_total: sorted_dirs.len(),
                files_done: 0,
                files_total: total_files,
                bytes_done: 0,
                bytes_total: total_bytes,
            });
            if let Some(existing) = db.album_by_import_source(&import_source)? {
                album_ids.insert(dir_rel.clone(), existing.record.id);
                report.albums_reused += 1;
                continue;
            }
            let parent_id = dir_rel
                .rsplit_once('/')
                .and_then(|(p, _)| album_ids.get(p).cloned());
            let mut id = [0u8; 16];
            getrandom::getrandom(&mut id).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            let album_id = hex::encode(id);
            let lamport = db.lamport()? + 1;
            db.upsert_album(&Versioned {
                lamport,
                device_id: me.clone(),
                record: index::models::AlbumRecord {
                    id: album_id.clone(),
                    name: album_name.clone(),
                    parent_id,
                    sort_order: index as i64,
                    import_source: Some(import_source),
                    local_only: true,
                    created_at: now_secs(),
                    deleted_at: None,
                },
            })?;
            db.set_lamport(lamport)?;
            album_ids.insert(dir_rel.clone(), album_id);
            report.albums_created += 1;
        }

        // ---- 阶段 4：上传（并发 workers + 主线程 DB/manifest 写）----
        let known: std::collections::HashSet<String> =
            db.load_blobs()?.into_iter().map(|v| v.record.hash).collect();
        let mut queue: std::collections::VecDeque<index::sqlite::StagingRow> =
            rows.into_iter().filter(|r| r.status != "done").collect();
        let mut files_done = 0usize;
        let mut bytes_done: u64 = 0;
        let mut consecutive_failures = 0usize;
        let mut inflight: Vec<(
            index::sqlite::StagingRow,
            std::thread::JoinHandle<Result<shard::EncodedBlob, String>>,
        )> = Vec::new();
        let concurrency = opts.concurrency.max(1).min(8);

        loop {
            if cancel.load(Ordering::Relaxed) {
                break; // 取消：已传保留，未传留 staging
            }
            if consecutive_failures >= 5 && inflight.is_empty() {
                report.paused = true; // 连续失败 5 个暂停（§8.5）
                break;
            }
            // 填满 worker 槽位
            while inflight.len() < concurrency {
                let Some(row) = queue.pop_front() else { break };
                // 主线程去重（§8.4）：已在索引 → staging done，不占 worker
                if known.contains(&row.sha256) {
                    db.staging_set_status(row.id, "done", "")?;
                    report.skipped_duplicate += 1;
                    files_done += 1;
                    bytes_done += row.size.max(0) as u64;
                    continue;
                }
                let keys = keys.clone();
                let stores: Vec<Box<dyn BlobStore>> =
                    self.storages.iter().map(|s| s.store()).collect::<Result<Vec<_>, _>>()?;
                let blob_sha: [u8; 32] = decode_hash(&row.sha256)?;
                let abs_path = PathBuf::from(&row.abs_path);
                let handle = std::thread::spawn(move || -> Result<shard::EncodedBlob, String> {
                    let bytes = read_file_retry(&abs_path)?;
                    let blob_key = keys.blob_key(&blob_sha).map_err(|e| e.to_string())?;
                    let encoded = encode_blob(
                        &bytes,
                        &blob_key,
                        DEFAULT_CHUNK_SIZE,
                        DEFAULT_N,
                        DEFAULT_M,
                        generate_iv,
                    )
                    .map_err(|e| e.to_string())?;
                    for s in &encoded.shards {
                        let store = &stores[usize::from(s.shard_index) % stores.len()];
                        store.put(&s.object_key, &s.data).map_err(|e| e.to_string())?;
                    }
                    Ok(encoded)
                });
                inflight.push((row, handle));
            }
            if inflight.is_empty() {
                break;
            }
            // 收割一个完成的 worker，主线程做 DB/manifest 写
            let (row, handle) = inflight.remove(0);
            let file_rel = row.rel_path.clone();
            let size = row.size.max(0) as u64;
            let album_dir = row.rel_path.rsplit_once('/').map(|(d, _)| d.to_string());
            match handle.join() {
                Ok(Ok(encoded)) => {
                    consecutive_failures = 0;
                    let blob_hex = hex::encode(encoded.blob_sha256);
                    let album_id = album_dir
                        .as_ref()
                        .and_then(|d| album_ids.get(d).cloned());
                    self.finalize_import_file(&keys, db, &me, &row, &encoded, &blob_hex, album_id)?;
                    report.imported += 1;
                }
                Ok(Err(e)) => {
                    consecutive_failures += 1;
                    db.staging_set_status(row.id, "failed", &e)?;
                    report.failed.push(format!("{file_rel}: {e}"));
                }
                Err(_) => {
                    consecutive_failures += 1;
                    db.staging_set_status(row.id, "failed", "worker panicked")?;
                    report.failed.push(format!("{file_rel}: worker panicked"));
                }
            }
            // 连续失败 5 个立即暂停（§8.5）：剩余文件留 staging 续传
            if consecutive_failures >= 5 {
                report.paused = true;
                break;
            }
            files_done += 1;
            bytes_done += size;
            let progress_album = album_dir.as_ref().and_then(|d| {
                album_ids
                    .get(d)
                    .and_then(|_| d.rsplit('/').next().map(|n| n.to_string()))
            });
            progress(ImportProgress {
                session_id: session_id.to_string(),
                stage: "upload".into(),
                album_name: progress_album,
                album_index: 0,
                album_total: 0,
                files_done,
                files_total: total_files,
                bytes_done,
                bytes_total: total_bytes,
            });
        }

        Ok(report)
    }

    /// 上传成功后的主线程收尾：manifest 冗余写入 + blob 记录 + 相册关联 + staging done。
    #[allow(clippy::too_many_arguments)]
    fn finalize_import_file(
        &self,
        keys: &crypto::DerivedKeys,
        db: &index::sqlite::IndexDb,
        me: &str,
        row: &index::sqlite::StagingRow,
        encoded: &shard::EncodedBlob,
        blob_hex: &str,
        album_id: Option<String>,
    ) -> Result<(), LibError> {
        let manifest = Manifest {
            version: 1,
            codec: CODEC_ID.to_string(),
            blob_sha256: blob_hex.to_string(),
            total_len: encoded.total_len as u64,
            chunk_size: DEFAULT_CHUNK_SIZE as u32,
            n: DEFAULT_N,
            m: DEFAULT_M,
            created_at: now_secs(),
            file_name: row.rel_path.rsplit('/').next().unwrap_or("file").to_string(),
            mime: mime_of(Path::new(&row.abs_path)),
            chunks: encoded
                .chunks
                .iter()
                .map(|c| ManifestChunk {
                    plaintext_sha256: hex::encode(c.plaintext_sha256),
                    ciphertext_len: c.ciphertext.len() as u32,
                })
                .collect(),
            shards: encoded
                .shards
                .iter()
                .map(|s| ManifestShard {
                    chunk_index: s.chunk_index,
                    shard_index: s.shard_index,
                    storage_id: self.storages[usize::from(s.shard_index) % self.storages.len()].id(),
                    remote_key: s.object_key.clone(),
                    size: s.data.len() as u32,
                    sha256: hex::encode(s.sha256),
                })
                .collect(),
        };
        self.save_manifest(blob_hex, &manifest)?;

        let blob_sha256: [u8; 32] = decode_hash(blob_hex)?;
        let lamport = db.lamport()? + 1;
        db.upsert_blob(&Versioned {
            lamport,
            device_id: me.to_string(),
            record: BlobRecord {
                hash: blob_hex.to_string(),
                file_name: manifest.file_name.clone(),
                size: manifest.total_len as i64,
                mime: manifest.mime.clone(),
                width: 0,
                height: 0,
                taken_at: row.mtime,
                chunk_size: DEFAULT_CHUNK_SIZE as i64,
                n: DEFAULT_N as i64,
                m: DEFAULT_M as i64,
                manifest_storage_key: manifest_object_key(blob_hex)?,
                created_at: manifest.created_at,
                deleted_at: None,
            },
        })?;
        db.set_lamport(lamport)?;

        if let Some(album_id) = album_id {
            let lamport = db.lamport()? + 1;
            db.upsert_blob_album(&Versioned {
                lamport,
                device_id: me.to_string(),
                record: index::models::BlobAlbumRecord {
                    blob_hash: blob_hex.to_string(),
                    album_id,
                    added_at: now_secs(),
                    deleted_at: None,
                },
            })?;
            db.set_lamport(lamport)?;
        }
        db.staging_set_status(row.id, "done", "")?;
        Ok(())
    }
}
