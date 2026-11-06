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
use zeroize::Zeroizing;
use ida::CODEC_ID;
use index::models::{BlobRecord, Versioned};
use index::sqlite::IndexDb;
use index::store::BlobStore;
use index::sync;
use keypath::manifest_object_key;
use serde::{Deserialize, Serialize};
use shard::{encode_blob, ChunkManifestInfo};
use thiserror::Error;

use crate::storage_ftp::{FtpConfig, FtpStore};
use crate::storage_openlist::{OpenListConfig, OpenListStore};
use crate::storage_s3::{S3Config, S3Store};
use crate::storage_webdav::{WebDavConfig, WebDavStore};

pub const DEFAULT_CHUNK_SIZE: usize = shard::DEFAULT_CHUNK_SIZE; // 1 MiB
pub const DEFAULT_N: u8 = shard::DEFAULT_N; // 4
pub const DEFAULT_M: u8 = shard::DEFAULT_M; // 2

#[derive(Debug, Error)]
pub enum LibError {
    #[error("文件读写错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("加密解密失败: {0}")]
    Crypto(#[from] CryptoError),
    #[error("分片处理失败: {0}")]
    Shard(#[from] shard::ShardError),
    #[error("存储路径错误: {0}")]
    Keypath(#[from] keypath::KeypathError),
    #[error("数据序列化失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("索引操作失败: {0}")]
    Index(#[from] index::IndexError),
    #[error("存储访问失败: {0}")]
    Store(#[from] index::store::StoreError),
    #[error("纠删码处理失败: {0}")]
    Ida(#[from] ida::IdaError),
    #[error("请先解锁")]
    Locked,
    #[error("blob {0} 不在索引中")]
    BlobNotFound(String),
    #[error("manifest 缺失或不可读: {0}")]
    ManifestMissing(String),
    #[error("数据错误: {0}")]
    Data(String),
    #[error("chunk {0} 无法解码/解密")]
    ChunkCorrupted(u32),
}

/// 会话期持有派生密钥与打开的加密索引；Drop 时零化。
pub struct HeartCore {
    data_dir: PathBuf,
    keys: Option<DerivedKeys>,
    db: Option<IndexDb>,
    storages: Vec<StorageConfig>,
    settings: Settings,
}

/// 时间线/列表条目。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlobEntry {
    pub blob_hash: String,
    pub file_name: String,
    pub mime: String,
    pub total_len: i64,
    pub taken_at: i64,
    /// EXIF GPS（§10 地图视图；无 GPS 为 None）
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
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
    /// 全部 shard 健康且 ≥ m 可恢复
    pub ok: bool,
    /// 可恢复（可用 shard ≥ m）但非全部健康
    pub degraded: bool,
    /// 不可恢复（可用 shard < m）
    pub unrecoverable: bool,
    pub available: usize,
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
    WebDav(WebDavConfig),
    Ftp(FtpConfig),
}

impl StorageConfig {
    pub fn id(&self) -> String {
        match self {
            StorageConfig::LocalDir { id, .. } => id.clone(),
            StorageConfig::S3(c) => c.bucket.clone(),
            StorageConfig::OpenList(c) => c.base_url.clone(),
            StorageConfig::WebDav(c) => c.base_url.clone(),
            StorageConfig::Ftp(c) => format!("{}:{}", c.host, c.port),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            StorageConfig::LocalDir { .. } => "local-dir",
            StorageConfig::S3(_) => "s3",
            StorageConfig::OpenList(_) => "openlist",
            StorageConfig::WebDav(_) => "webdav",
            StorageConfig::Ftp(_) => "ftp",
        }
    }

    pub fn store(&self) -> Result<Box<dyn BlobStore>, LibError> {
        match self {
            StorageConfig::LocalDir { id, root } => {
                Ok(Box::new(index::store::LocalDirStore::new(root.clone(), id.clone())))
            }
            StorageConfig::S3(c) => Ok(Box::new(S3Store::new(c.clone())?)),
            StorageConfig::OpenList(c) => Ok(Box::new(OpenListStore::new(c.clone())?)),
            StorageConfig::WebDav(c) => Ok(Box::new(WebDavStore::new(c.clone())?)),
            StorageConfig::Ftp(c) => Ok(Box::new(FtpStore::new(c.clone())?)),
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

/// MK 封装文件（§10 修改口令=重加密封装，不重传 shard）：
/// `wrapped = encrypt_bare(KEK, iv, MK(32B) ‖ master_salt(32B))`，
/// KEK = Argon2id(口令, kdf_salt)。子密钥全部从 MK 派生 → 改口令只重写本文件。
#[derive(Debug, Serialize, Deserialize)]
struct MkWrapFile {
    v: u32,
    kdf_salt_hex: String,
    m_cost_kib: u32,
    t_cost: u32,
    parallelism: u32,
    wrapped_hex: String,
}

/// §10 设置（非敏感参数；明文落盘 settings.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 默认分片大小（256 KiB – 4 MiB；只影响新上传）
    pub chunk_size: usize,
    /// 纠删码总 shard 数
    pub n: u8,
    /// 恢复阈值
    pub m: u8,
    /// 下载（导出）并发数
    pub download_concurrency: usize,
    /// 主题：dark | light
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            n: DEFAULT_N,
            m: DEFAULT_M,
            download_concurrency: 3,
            theme: "dark".into(),
        }
    }
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
        Self {
            data_dir: data_dir.into(),
            keys: None,
            db: None,
            storages: Vec::new(),
            settings: Settings::default(),
        }
    }

    fn kdf_params_path(&self) -> PathBuf {
        self.data_dir.join("kdf.params.json")
    }

    fn mk_wrap_path(&self) -> PathBuf {
        self.data_dir.join("mk.wrap")
    }

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
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
    // 会话（MK 封装架构：随机 MK + 口令 KEK 封装，改口令只重写封装）
    // -----------------------------------------------------------------------

    fn load_or_init_settings(&mut self) {
        if self.settings_path().exists() {
            if let Ok(raw) = fs::read(self.settings_path()) {
                if let Ok(s) = serde_json::from_slice::<Settings>(&raw) {
                    self.settings = s;
                }
            }
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_settings(&mut self, s: Settings) -> Result<(), LibError> {
        self.settings = s;
        fs::write(self.settings_path(), serde_json::to_vec_pretty(&self.settings)?)?;
        Ok(())
    }

    /// 解封 MK：KEK = Argon2id(口令, kdf_salt) → decrypt_bare(wrapped) → MK‖master_salt。
    fn unwrap_mk(wrap: &MkWrapFile, password: &str) -> Result<(Zeroizing<[u8; 32]>, [u8; 32]), LibError> {
        let kdf_salt = hex::decode(&wrap.kdf_salt_hex)
            .map_err(|e| LibError::Data(format!("kdf salt corrupt: {e}")))?;
        let params = KdfParams {
            m_cost_kib: wrap.m_cost_kib,
            t_cost: wrap.t_cost,
            parallelism: wrap.parallelism,
        };
        let kek = derive_master_key(password.as_bytes(), &kdf_salt, &params)?;
        let wrapped = hex::decode(&wrap.wrapped_hex)
            .map_err(|e| LibError::Data(format!("mk wrap corrupt: {e}")))?;
        let payload = decrypt_bare(kek.as_ref(), &wrapped)
            .map_err(|_| LibError::Data("wrong password (MK unwrap failed)".into()))?;
        if payload.len() != 64 {
            return Err(LibError::Data("mk wrap payload corrupt".into()));
        }
        let mut mk = Zeroizing::new([0u8; 32]);
        mk.as_mut().copy_from_slice(&payload[..32]);
        let mut master_salt = [0u8; 32];
        master_salt.copy_from_slice(&payload[32..]);
        Ok((mk, master_salt))
    }

    pub fn unlock(&mut self, password: &str) -> Result<(), LibError> {
        self.load_or_init_settings();

        // MK 封装：首次生成随机 MK + 随机 kdf 盐；之后每次解锁只做解封。
        // 改口令 = 用新 KEK 重加密同一 MK（change_password），shard/索引全不动。
        let wrap: MkWrapFile = if self.mk_wrap_path().exists() {
            let raw = fs::read(self.mk_wrap_path())?;
            serde_json::from_slice::<MkWrapFile>(&raw)
                .map_err(|e| LibError::Data(format!("mk wrap corrupt: {e}")))?
        } else {
            fs::create_dir_all(&self.data_dir)?;
            let mut mk = Zeroizing::new([0u8; 32]);
            getrandom::getrandom(mk.as_mut()).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            let mut ms = [0u8; 32];
            getrandom::getrandom(&mut ms).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            let mut kdf_salt = [0u8; 16];
            getrandom::getrandom(&mut kdf_salt).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            let p = KdfParams::standard();
            let kek = derive_master_key(password.as_bytes(), &kdf_salt, &p)?;
            let mut payload = [0u8; 64];
            payload[..32].copy_from_slice(mk.as_ref());
            payload[32..].copy_from_slice(&ms);
            let iv = generate_iv()?;
            let wrapped = encrypt_bare(kek.as_ref(), &iv, &payload)?;
            let f = MkWrapFile {
                v: 1,
                kdf_salt_hex: hex::encode(kdf_salt),
                m_cost_kib: p.m_cost_kib,
                t_cost: p.t_cost,
                parallelism: p.parallelism,
                wrapped_hex: hex::encode(wrapped),
            };
            fs::write(self.mk_wrap_path(), serde_json::to_vec_pretty(&f)?)?;
            f
        };

        let (mk, master_salt) = Self::unwrap_mk(&wrap, password)?;
        let keys = DerivedKeys::new(mk, &master_salt)?;
        self.keys = Some(keys.clone());

        // 打开 SQLCipher 索引（错口令在解封 MK 时即失败）
        let index_key_hex = hex::encode(keys.index_key.as_ref());
        let db = match IndexDb::open(self.index_db_path(), &index_key_hex) {
            Ok(db) => db,
            Err(index::IndexError::Sqlite(e)) if e.to_string().contains("not a database") => {
                self.keys = None;
                return Err(LibError::Data(
                    "口令错误（索引解密失败）".into(),
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

    /// §10 修改口令：旧口令解封 MK → 新 KEK 重加密封装 → 写回。
    /// 子密钥全部从 MK 派生 → shard/manifest/索引完全不动（不重传 shard）。
    pub fn change_password(&self, old_password: &str, new_password: &str) -> Result<(), LibError> {
        let raw = fs::read(self.mk_wrap_path())?;
        let wrap: MkWrapFile = serde_json::from_slice::<MkWrapFile>(&raw)
            .map_err(|e| LibError::Data(format!("mk wrap corrupt: {e}")))?;
        let (_mk, _ms) = Self::unwrap_mk(&wrap, old_password)?; // 验证旧口令

        let mut new_kdf_salt = [0u8; 16];
        getrandom::getrandom(&mut new_kdf_salt).map_err(|e| LibError::Data(format!("rng: {e}")))?;
        let params = KdfParams {
            m_cost_kib: wrap.m_cost_kib,
            t_cost: wrap.t_cost,
            parallelism: wrap.parallelism,
        };
        let new_kek = derive_master_key(new_password.as_bytes(), &new_kdf_salt, &params)?;

        // 用旧口令重新解封拿到原始 payload（MK‖master_salt），避免依赖会话状态
        let kdf_salt = hex::decode(&wrap.kdf_salt_hex).map_err(|e| LibError::Data(format!("{e}")))?;
        let old_kek = derive_master_key(old_password.as_bytes(), &kdf_salt, &params)?;
        let wrapped = hex::decode(&wrap.wrapped_hex).map_err(|e| LibError::Data(format!("{e}")))?;
        let payload = decrypt_bare(old_kek.as_ref(), &wrapped)?;

        let iv = generate_iv()?;
        let new_wrapped = encrypt_bare(new_kek.as_ref(), &iv, &payload)?;
        let new_wrap = MkWrapFile {
            v: 1,
            kdf_salt_hex: hex::encode(new_kdf_salt),
            m_cost_kib: params.m_cost_kib,
            t_cost: params.t_cost,
            parallelism: params.parallelism,
            wrapped_hex: hex::encode(new_wrapped),
        };
        let tmp = self.mk_wrap_path().with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&new_wrap)?)?;
        fs::rename(&tmp, self.mk_wrap_path())?;
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

        // §10 EXIF 解析（图片文件）
        let exif_data = if mime_of(path).starts_with("image/") {
            parse_exif_from_bytes(&blob)
        } else {
            ExifData::default()
        };

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
        let s = self.settings.clone();
        let encoded = encode_blob(&blob, &blob_key, s.chunk_size, s.n, s.m, generate_iv)?;

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
            chunk_size: s.chunk_size as u32,
            n: s.n,
            m: s.m,
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
                width: exif_data.width.unwrap_or(0),
                height: exif_data.height.unwrap_or(0),
                gps_lat: exif_data.gps_lat,
                gps_lon: exif_data.gps_lon,
                taken_at: exif_data.taken_at.unwrap_or(taken_at),
                chunk_size: s.chunk_size as i64,
                n: s.n as i64,
                m: s.m as i64,
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
                gps_lat: v.record.gps_lat,
                gps_lon: v.record.gps_lon,
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

    /// §11 Range 下载：只拉取覆盖 `[offset, offset+len)` 字节区间的 chunk 分片，
    /// 解密后截取写入。用于大图预览/局部读取，不落整文件。
    pub fn export_blob_range(
        &self,
        blob_hash: &str,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, LibError> {
        let keys = self.keys_or_err()?.clone();
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let chunk_size = manifest.chunk_size as u64;
        if len == 0 {
            return Ok(Vec::new());
        }
        // 覆盖区间 [offset, offset+len) 的 chunk 序号 [first, last]
        let first_chunk = offset / chunk_size;
        let last_chunk = (offset + len - 1) / chunk_size;
        if last_chunk >= manifest.chunks.len() as u64 {
            return Err(LibError::Data("range beyond blob end".into()));
        }

        let blob_hash_bytes: [u8; 32] = decode_hash(&manifest.blob_sha256)?;
        let blob_key = keys.blob_key(&blob_hash_bytes)?;
        let total_chunks = manifest.chunks.len() as u32;
        let mut out: Vec<u8> = Vec::with_capacity(len as usize);

        for ci in first_chunk..=last_chunk {
            let ci = ci as u32;
            let info = &manifest.chunks[ci as usize];
            let ct_len = info.ciphertext_len as usize;
            let chunk_infos = [ChunkManifestInfo {
                plaintext_sha256: decode_hash(&info.plaintext_sha256)?,
                ciphertext_len: ct_len,
            }];
            let mut available: Vec<(u8, Vec<u8>)> = Vec::new();
            for s in manifest.shards.iter().filter(|s| s.chunk_index == ci) {
                if let Some(store) = self.store_by_id(&s.storage_id).ok() {
                    if let Some(data) = store.get(&s.remote_key) {
                        available.push((s.shard_index, data));
                    }
                }
            }
            if available.len() < usize::from(manifest.m) {
                return Err(LibError::Data(format!("chunk {ci}: not enough shards for range read")));
            }
            available.sort_by_key(|(i, _)| *i);
            let refs: Vec<(u8, &[u8])> =
                available.iter().map(|(i, d)| (*i, d.as_slice())).collect();
            let ciphertext = ida::decode_shards(&refs, config_for(manifest.n, manifest.m)?, ct_len)
                .map_err(|_| LibError::ChunkCorrupted(ci))?;
            let plaintext = crypto::decrypt_chunk(
                &blob_key,
                &ciphertext,
                &blob_hash_bytes,
                ci,
                total_chunks,
            )
            .map_err(|_| LibError::ChunkCorrupted(ci))?;
            let chunk_start = ci as u64 * chunk_size;
            // 本 chunk 在输出区间的起止
            let read_from = offset.saturating_sub(chunk_start) as usize;
            let read_to = std::cmp::min(plaintext.len(), (offset + len - chunk_start) as usize);
            if read_from < plaintext.len() && read_to > read_from {
                out.extend_from_slice(&plaintext[read_from..read_to]);
            }
        }
        Ok(out)
    }

    // ---------------- §10 相册树命令 ----------------

    /// 相册树数据（未删除 + blob 计数 + 封面）。
    pub fn list_albums(&self) -> Result<Vec<AlbumInfo>, LibError> {        let db = self.db_or_err()?;
        let mut albums: Vec<AlbumInfo> = db
            .load_albums()?
            .into_iter()
            .filter(|v| v.record.deleted_at.is_none())
            .map(|v| AlbumInfo {
                id: v.record.id.clone(),
                name: v.record.name,
                parent_id: v.record.parent_id,
                sort_order: v.record.sort_order,
                cover_hash: v.record.cover_hash,
                blob_count: 0,
            })
            .collect();
        let stats = db.album_stats()?;
        for a in &mut albums {
            if let Some((_, cnt, _)) = stats.iter().find(|(id, _, _)| id == &a.id) {
                a.blob_count = *cnt;
            }
        }
        albums.sort_by_key(|a| (a.sort_order, a.name.clone()));
        Ok(albums)
    }

    /// 创建相册（手动）。
    pub fn album_create(&self, name: &str, parent_id: Option<String>) -> Result<String, LibError> {
        let db = self.db_or_err()?;
        let me = db.device_id()?;
        let mut id = [0u8; 16];
        getrandom::getrandom(&mut id).map_err(|e| LibError::Data(format!("rng: {e}")))?;
        let album_id = hex::encode(id);
        let lamport = db.lamport()? + 1;
        db.upsert_album(&Versioned {
            lamport,
            device_id: me,
            record: index::models::AlbumRecord {
                id: album_id.clone(),
                name: name.to_string(),
                parent_id,
                sort_order: 0,
                import_source: None,
                local_only: false,
                cover_hash: String::new(),
                created_at: now_secs(),
                deleted_at: None,
            },
        })?;
        db.set_lamport(lamport)?;
        Ok(album_id)
    }

    /// 重命名相册。
    pub fn album_rename(&self, album_id: &str, name: &str) -> Result<(), LibError> {
        let db = self.db_or_err()?;
        let me = db.device_id()?;
        let existing = db
            .load_albums()?
            .into_iter()
            .find(|v| v.record.id == album_id)
            .ok_or_else(|| LibError::Data(format!("album {album_id} not found")))?;
        let lamport = db.lamport()? + 1;
        let mut record = existing.record.clone();
        record.name = name.to_string();
        db.upsert_album(&Versioned { lamport, device_id: me, record })?;
        db.set_lamport(lamport)?;
        Ok(())
    }

    /// 拖拽排序/移动：更新 sort_order（可选同时改 parent）。
    pub fn album_set_sort(
        &self,
        album_id: &str,
        sort_order: i64,
        parent_id: Option<String>,
    ) -> Result<(), LibError> {
        let db = self.db_or_err()?;
        let me = db.device_id()?;
        let existing = db
            .load_albums()?
            .into_iter()
            .find(|v| v.record.id == album_id)
            .ok_or_else(|| LibError::Data(format!("album {album_id} not found")))?;
        let lamport = db.lamport()? + 1;
        let mut record = existing.record.clone();
        record.sort_order = sort_order;
        if parent_id.is_some() {
            record.parent_id = parent_id;
        }
        db.upsert_album(&Versioned { lamport, device_id: me, record })?;
        db.set_lamport(lamport)?;
        Ok(())
    }

    /// §10 封面设置。
    pub fn album_set_cover(&self, album_id: &str, blob_hash: &str) -> Result<(), LibError> {
        let db = self.db_or_err()?;
        let me = db.device_id()?;
        let lamport = db.lamport()? + 1;
        db.album_set_cover(album_id, &blob_hash.to_lowercase(), lamport, &me)?;
        db.set_lamport(lamport)?;
        Ok(())
    }

    /// §10 大图预览：1024px 缩略图（加密缓存，独立于列表 256px）。
    pub fn thumbnail_large(&self, blob_hash: &str) -> Result<String, LibError> {
        let keys = self.keys_or_err()?;
        const SIZE: u32 = 1024;
        let cache = self.data_dir.join("thumbs").join(format!("{blob_hash}.{SIZE}.enc"));
        if cache.exists() {
            let enc = fs::read(&cache)?;
            let png = decrypt_bare(keys.manifest_key.as_ref(), &enc)?;
            use base64::Engine as _;
            return Ok(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            ));
        }
        Err(LibError::Data("large thumbnail not cached yet".into()))
    }

    /// 生成并缓存大图（从导出的源文件）。
    pub fn thumbnail_large_put(&self, blob_hash: &str, src_path: &Path) -> Result<(), LibError> {
        let bytes = fs::read(src_path)?;
        let png = Self::generate_thumbnail_from_bytes(&bytes, 1024)?;
        let keys = self.keys_or_err()?;
        let thumb_dir = self.data_dir.join("thumbs");
        fs::create_dir_all(&thumb_dir)?;
        let cache = thumb_dir.join(format!("{blob_hash}.1024.enc"));
        let enc = encrypt_bare(keys.manifest_key.as_ref(), &generate_iv()?, &png)?;
        let tmp = cache.with_extension("tmp");
        fs::write(&tmp, &enc)?;
        fs::rename(&tmp, &cache)?;
        Ok(())
    }

    pub fn verify_blob(&self, blob_hash: &str) -> Result<VerifyReport, LibError> {
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let mut missing = Vec::new();
        let mut corrupted = Vec::new();
        let mut available = 0usize;
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
                    } else {
                        available += 1;
                    }
                }
            }
        }
        let shard_count = manifest.shards.len();
        // 体检语义：全健康→ok；可用 ≥ m 可恢复但非全健康→degraded（黄色）；可用 < m→unrecoverable（红色）
        let ok = missing.is_empty() && corrupted.is_empty();
        let recoverable = available >= manifest.m as usize;
        Ok(VerifyReport {
            blob_hash: blob_hex,
            ok,
            degraded: !ok && recoverable,
            unrecoverable: !recoverable,
            missing,
            corrupted,
            shard_count,
            available,
        })
    }

    /// §10 点击预览：解密 blob → 写入临时文件 → 返回路径和 MIME。
    pub fn preview_blob(&self, blob_hash: &str) -> Result<(PathBuf, String), LibError> {
        let keys = self.keys_or_err()?.clone();
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let blob_hash_bytes: [u8; 32] = decode_hash(&manifest.blob_sha256)?;
        let blob_key = keys.blob_key(&blob_hash_bytes)?;

        let mut chunk_infos = Vec::with_capacity(manifest.chunks.len());
        for c in &manifest.chunks {
            chunk_infos.push(ChunkManifestInfo {
                plaintext_sha256: decode_hash(&c.plaintext_sha256)?,
                ciphertext_len: c.ciphertext_len as usize,
            });
        }

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

        if crypto::sha256(&decoded) != blob_hash_bytes {
            return Err(LibError::Data("SHA-256 mismatch".into()));
        }

        let ext = manifest
            .file_name
            .rsplit('.')
            .next()
            .unwrap_or("bin");
        let tmp = std::env::temp_dir()
            .join(format!("heartshard-preview-{}.{}", &blob_hex[..12], ext));
        fs::write(&tmp, &decoded)?;
        Ok((tmp, manifest.mime.clone()))
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

    /// 注册 WebDAV Storage（nginx dav / Nextcloud / Alist 等；Basic 认证）。
    pub fn add_storage_webdav(&mut self, config: WebDavConfig) -> Result<String, LibError> {
        let keys = self.keys_or_err()?.clone();
        let id = config.base_url.clone();
        if self.storages.iter().any(|s| s.id() == id) {
            return Err(LibError::Data(format!("storage {id} already registered")));
        }
        self.storages.push(StorageConfig::WebDav(config));
        self.save_registry(&keys)?;
        Ok(id)
    }

    /// 注册 FTPS Storage（明文 FTP 默认禁用——凭据明文传输，仅限可信内网）。
    pub fn add_storage_ftp(&mut self, config: FtpConfig) -> Result<String, LibError> {
        let keys = self.keys_or_err()?.clone();
        let id = format!("{}:{}", config.host, config.port);
        if self.storages.iter().any(|s| s.id() == id) {
            return Err(LibError::Data(format!("storage {id} already registered")));
        }
        self.storages.push(StorageConfig::Ftp(config));
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
                // 写到任意一个**当前可用**的 Storage（原存储可能已移除）
                let mut written = false;
                for cfg in &self.storages {
                    let store = cfg.store()?;
                    if store.put(&s.remote_key, &parts[usize::from(shard_index)]).is_ok() {
                        repaired.push(format!("[{}] {}", cfg.id(), s.remote_key));
                        written = true;
                        break;
                    }
                }
                if !written {
                    unrecoverable.push(chunk_index);
                }
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

    // ---------------- §10/§11 补充：缩略图 / 详情 / 一键体检 / 历史报告 ----------------

    /// 缩略图缓存读取（PNG data URL）。缓存以 manifest_key 加密于
    /// `{data_dir}/thumbs/{hash}.{size}.enc`（§11：本地生成并加密缓存）。
    pub fn thumbnail_cached(&self, blob_hash: &str) -> Result<Option<String>, LibError> {
        let keys = self.keys_or_err()?;
        const SIZE: u32 = 256;
        let cache = self.data_dir.join("thumbs").join(format!("{blob_hash}.{SIZE}.enc"));
        if !cache.exists() {
            return Ok(None);
        }
        let enc = fs::read(&cache)?;
        let png = decrypt_bare(keys.manifest_key.as_ref(), &enc)?;
        use base64::Engine as _;
        Ok(Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        )))
    }

    /// 缩略图加密缓存写入（导入时由 worker 生成的 PNG 调用）。
    pub fn thumbnail_cache_put(&self, blob_hash: &str, png: &[u8]) -> Result<(), LibError> {
        let keys = self.keys_or_err()?;
        const SIZE: u32 = 256;
        let thumb_dir = self.data_dir.join("thumbs");
        fs::create_dir_all(&thumb_dir)?;
        let cache = thumb_dir.join(format!("{blob_hash}.{SIZE}.enc"));
        let enc = encrypt_bare(keys.manifest_key.as_ref(), &generate_iv()?, png)?;
        let tmp = cache.with_extension("tmp");
        fs::write(&tmp, &enc)?;
        fs::rename(&tmp, &cache)?;
        Ok(())
    }

    /// 从已读入内存的图片字节生成缩略图 PNG。
    pub fn generate_thumbnail_from_bytes(bytes: &[u8], size: u32) -> Result<Vec<u8>, LibError> {
        let img = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| LibError::Data(format!("open image: {e}")))?
            .decode()
            .map_err(|e| LibError::Data(format!("decode image: {e}")))?;
        let thumb = img.thumbnail(size, size);
        let mut out = std::io::Cursor::new(Vec::new());
        thumb
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| LibError::Data(format!("encode thumb: {e}")))?;
        Ok(out.into_inner())
    }

    /// blob 详情：manifest 中的分片分布（按 Storage 汇总，可视化数据）。
    pub fn blob_detail(&self, blob_hash: &str) -> Result<BlobDetail, LibError> {
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let mut per_storage: std::collections::BTreeMap<String, usize> = Default::default();
        let shards = manifest
            .shards
            .iter()
            .map(|s| {
                *per_storage.entry(s.storage_id.clone()).or_default() += 1;
                ShardPlacement {
                    chunk_index: s.chunk_index,
                    shard_index: s.shard_index,
                    storage_id: s.storage_id.clone(),
                    remote_key: s.remote_key.clone(),
                    size: s.size,
                }
            })
            .collect();
        Ok(BlobDetail {
            blob_hash: blob_hex,
            file_name: manifest.file_name,
            mime: manifest.mime,
            total_len: manifest.total_len as i64,
            taken_at: manifest.created_at,
            chunk_size: manifest.chunk_size,
            n: manifest.n,
            m: manifest.m,
            chunks: manifest.chunks.len(),
            shards,
            per_storage: per_storage.into_iter().collect(),
        })
    }

    /// 一键体检（§10）：对索引中全部 blob 逐一 verify。
    pub fn verify_all(&self) -> Result<Vec<VerifyReport>, LibError> {
        let mut out = Vec::new();
        for b in self.list_blobs()? {
            out.push(self.verify_blob(&b.blob_hash)?);
        }
        Ok(out)
    }

    /// 导入运行报告落盘（§10.2 历史报告可重开）。
    pub fn save_import_report(&self, report: &ImportRunReport) -> Result<String, LibError> {
        let dir = self.data_dir.join("import-reports");
        fs::create_dir_all(&dir)?;
        let name = format!(
            "import-report-{}-{}.json",
            report.session_id,
            now_secs()
        );
        fs::write(dir.join(&name), serde_json::to_vec_pretty(report)?)?;
        Ok(name)
    }

    /// 历史报告列表（文件名新→旧）。
    pub fn import_history(&self) -> Result<Vec<String>, LibError> {
        let dir = self.data_dir.join("import-reports");
        let mut names: Vec<String> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".json"))
            .collect();
        names.sort_by(|a, b| b.cmp(a));
        Ok(names)
    }

    /// 重开一份历史报告（只允许纯文件名，防路径逃逸）。
    pub fn import_report_get(&self, file: &str) -> Result<ImportRunReport, LibError> {
        if file.contains('/') || file.contains('\\') || file.contains("..") {
            return Err(LibError::Data("bad report file name".into()));
        }
        let raw = fs::read(self.data_dir.join("import-reports").join(file))?;
        serde_json::from_slice(&raw).map_err(|e| LibError::Data(format!("report corrupt: {e}")))
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

/// 单个 shard 的分布信息（来自 manifest；健康状态由 verify_blob 单独给出）。
#[derive(Debug, Clone, Serialize)]
pub struct ShardPlacement {
    pub chunk_index: u32,
    pub shard_index: u8,
    pub storage_id: String,
    pub remote_key: String,
    pub size: u32,
}

/// §10 相册树条目。
#[derive(Debug, Clone, Serialize)]
pub struct AlbumInfo {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
    pub cover_hash: String,
    pub blob_count: i64,
}

fn config_for(n: u8, m: u8) -> Result<ida::DispersalConfig, LibError> {
    ida::DispersalConfig::new(n, m).map_err(LibError::from)
}

#[derive(Debug, Clone, Serialize)]
pub struct BlobDetail {
    pub blob_hash: String,
    pub file_name: String,
    pub mime: String,
    pub total_len: i64,
    /// 拍摄/修改时间（EXIF DateTimeOriginal 解析在 M1.5；当前为文件 mtime）
    pub taken_at: i64,
    pub chunk_size: u32,
    pub n: u8,
    pub m: u8,
    pub chunks: usize,
    pub shards: Vec<ShardPlacement>,
    /// 每个 Storage 上的 shard 数（分片分布可视化数据）
    pub per_storage: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// EXIF 解析结果（§10 详情 + 地图视图）。
#[derive(Debug, Default, Clone)]
pub struct ExifData {
    pub taken_at: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

/// 从图片字节解析 EXIF DateTimeOriginal / PixelXDimension / GPS。
/// 非 JPEG 或无 EXIF 返回 Default。
fn parse_exif_from_bytes(bytes: &[u8]) -> ExifData {
    let mut out = ExifData::default();
    let mut cursor = std::io::Cursor::new(bytes);
    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut cursor) else { return out };

    // DateTimeOriginal → unix ts（ASCII 值 "YYYY:MM:DD HH:MM:SS"）
    if let Some(dt) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        if let exif::Value::Ascii(ref strings) = dt.value {
            if let Some(bytes) = strings.first() {
                let s = String::from_utf8_lossy(bytes);
                let parts: Vec<&str> = s.split(' ').collect();
                if parts.len() == 2 {
                    let date: Vec<u32> = parts[0].split(':').filter_map(|p| p.parse().ok()).collect();
                    let time: Vec<u32> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
                    if date.len() == 3 && time.len() == 3 {
                        let (y, mo, d) = (date[0] as i64, date[1], date[2]);
                        let days = days_from_civil(y, mo, d);
                        out.taken_at = Some(
                            days * 86_400
                                + (time[0] as i64) * 3600
                                + (time[1] as i64) * 60
                                + time[2] as i64,
                        );
                    }
                }
            }
        }
    }

    // PixelXDimension / PixelYDimension
    if let Some(w) = exif.get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY) {
        if let exif::Value::Long(ref v) = w.value {
            if let Some(&x) = v.first() {
                out.width = Some(x as i64);
            }
        }
    }
    if let Some(h) = exif.get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY) {
        if let exif::Value::Long(ref v) = h.value {
            if let Some(&y) = v.first() {
                out.height = Some(y as i64);
            }
        }
    }

    // GPS: 度/分/秒 有理数 + N/S/E/W 参考
    fn dms_to_deg(v: &exif::Value) -> Option<f64> {
        if let exif::Value::Rational(rationals) = v {
            if rationals.len() >= 3 {
                let d = rationals[0].to_f64();
                let m = rationals[1].to_f64();
                let s = rationals[2].to_f64();
                return Some(d + m / 60.0 + s / 3600.0);
            }
        }
        None
    }
    if let (Some(lat), Some(lat_ref), Some(lon), Some(lon_ref)) = (
        exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY),
    ) {
        let lat_is_south = match &lat_ref.value {
            exif::Value::Ascii(v) => v.first().map(|b| b.first() == Some(&b'S')).unwrap_or(false),
            _ => false,
        };
        let lon_is_west = match &lon_ref.value {
            exif::Value::Ascii(v) => v.first().map(|b| b.first() == Some(&b'W')).unwrap_or(false),
            _ => false,
        };
        if let (Some(mut la), Some(mut lo)) = (dms_to_deg(&lat.value), dms_to_deg(&lon.value)) {
            if lat_is_south { la = -la; }
            if lon_is_west { lo = -lo; }
            if la != 0.0 || lo != 0.0 {
                out.gps_lat = Some(la);
                out.gps_lon = Some(lo);
            }
        }
    }
    out
}

/// Howard Hinnant civil_from_days（1970-01-01 = day 0），仅 EXIF 解析用。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
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
                    cover_hash: String::new(),
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
            std::thread::JoinHandle<Result<(shard::EncodedBlob, Option<Vec<u8>>, ExifData), String>>,
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
                let settings = self.settings.clone();
                let stores: Vec<Box<dyn BlobStore>> =
                    self.storages.iter().map(|s| s.store()).collect::<Result<Vec<_>, _>>()?;
                let blob_sha: [u8; 32] = decode_hash(&row.sha256)?;
                let abs_path = PathBuf::from(&row.abs_path);
                let handle = std::thread::spawn(move || -> Result<(shard::EncodedBlob, Option<Vec<u8>>, ExifData), String> {
                    // 返回 3 元组：(编码结果, 缩略图, EXIF)
                    let bytes = read_file_retry(&abs_path)?;
                    // §11：图片文件顺手生成缩略图 PNG（懒加载 + 加密缓存用）
                    let thumb = if mime_of(&abs_path).starts_with("image/") {
                        Self::generate_thumbnail_from_bytes(&bytes, 256).ok()
                    } else {
                        None
                    };
                    // §10 EXIF：DateTimeOriginal / GPS / 尺寸
                    let exif_data = if mime_of(&abs_path).starts_with("image/") {
                        parse_exif_from_bytes(&bytes)
                    } else {
                        ExifData::default()
                    };
                    let blob_key = keys.blob_key(&blob_sha).map_err(|e| e.to_string())?;
                    let encoded = encode_blob(
                        &bytes,
                        &blob_key,
                        settings.chunk_size,
                        settings.n,
                        settings.m,
                        generate_iv,
                    )
                    .map_err(|e| e.to_string())?;
                    for s in &encoded.shards {
                        let store = &stores[usize::from(s.shard_index) % stores.len()];
                        store.put(&s.object_key, &s.data).map_err(|e| e.to_string())?;
                    }
                    Ok((encoded, thumb, exif_data))
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
                Ok(Ok((encoded, thumb, exif_data))) => {
                    consecutive_failures = 0;
                    let blob_hex = hex::encode(encoded.blob_sha256);
                    let album_id = album_dir
                        .as_ref()
                        .and_then(|d| album_ids.get(d).cloned());
                    self.finalize_import_file(&keys, db, &me, &row, &encoded, &blob_hex, album_id, &exif_data)?;
                    if let Some(png) = thumb {
                        let _ = self.thumbnail_cache_put(&blob_hex, &png);
                    }
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

        // 阶段 5 收尾：报告落盘（§10.2 历史报告可重开）
        let _ = self.save_import_report(&report);

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
        exif: &ExifData,
    ) -> Result<(), LibError> {
        let manifest = Manifest {
            version: 1,
            codec: CODEC_ID.to_string(),
            blob_sha256: blob_hex.to_string(),
            total_len: encoded.total_len as u64,
            chunk_size: self.settings.chunk_size as u32,
            n: self.settings.n,
            m: self.settings.m,
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
                width: exif.width.unwrap_or(0),
                height: exif.height.unwrap_or(0),
                taken_at: exif.taken_at.unwrap_or(row.mtime),
                gps_lat: exif.gps_lat,
                gps_lon: exif.gps_lon,
                chunk_size: self.settings.chunk_size as i64,
                n: self.settings.n as i64,
                m: self.settings.m as i64,
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
