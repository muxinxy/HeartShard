//! HeartShard M1 业务库：会话密钥、本地目录模拟 Storage、加密 manifest/索引、
//! 导入/导出/校验。与 Tauri 类型解耦，可直接被集成测试调用。
//!
//! M1 范围（简报）：上传 → 分片到本地目录（模拟 Storage）→ 下载还原 → SHA256 校验。
//! M2 起引入 SQLCipher index.db 与真实 S3/WebDAV 适配器，替换本文件中的
//! 加密 JSON 索引与本地目录 Storage。

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crypto::{
    decrypt_bare, derive_master_key, encrypt_bare, generate_iv, CryptoError, DerivedKeys,
    KdfParams,
};
use ida::CODEC_ID;
use keypath::manifest_object_key;
use serde::{Deserialize, Serialize};
use shard::{encode_blob, ChunkManifestInfo};
use thiserror::Error;

pub const DEFAULT_CHUNK_SIZE: usize = shard::DEFAULT_CHUNK_SIZE; // 1 MiB
pub const DEFAULT_N: u8 = shard::DEFAULT_N; // 4
pub const DEFAULT_M: u8 = shard::DEFAULT_M; // 2
/// M1 唯一的模拟 Storage id。
pub const MOCK_STORAGE_ID: &str = "local-mock";

#[derive(Debug, Error)]
pub enum LibError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("shard: {0}")]
    Shard(#[from] shard::ShardError),
    #[error("keypath: {0}")]
    Keypath(#[from] keypath::KeypathError),
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

/// 会话期持有派生密钥；Drop 时零化（MK 本体在 `DerivedKeys` 内）。
pub struct HeartCore {
    data_dir: PathBuf,
    keys: Option<DerivedKeys>,
}

/// 时间线/列表条目（解密后的本地索引内容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobEntry {
    pub blob_hash: String,
    pub file_name: String,
    pub mime: String,
    pub total_len: u64,
    /// 文件拍摄/修改时间（unix 秒）；EXIF 提取在 M1.5 引入。
    pub taken_at: i64,
    pub imported_at: i64,
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

// ---------------------------------------------------------------------------
// 磁盘上的固定格式
// ---------------------------------------------------------------------------

/// KDF 参数文件（非机密：盐与参数可落盘；口令与 MK 不落盘）。
#[derive(Debug, Serialize, Deserialize)]
struct KdfParamsFile {
    salt_hex: String,
    m_cost_kib: u32,
    t_cost: u32,
    parallelism: u32,
}

/// M1 加密 manifest（M2 起迁移到 core-rs/crates/manifest 的 serde 结构）。
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

/// M1 本地索引（MANIFEST_KEY 加密的 JSON；M2 换 SQLCipher index.db）。
#[derive(Debug, Default, Serialize, Deserialize)]
struct LocalIndex {
    blobs: Vec<BlobEntry>,
}

impl HeartCore {
    /// `data_dir`：生产环境为 app data 目录，测试传临时目录。
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into(), keys: None }
    }

    fn kdf_params_path(&self) -> PathBuf {
        self.data_dir.join("kdf.params.json")
    }

    fn index_path(&self) -> PathBuf {
        self.data_dir.join("index.m1.json.enc")
    }

    /// 模拟 Storage 根目录：`{data_dir}/storages/{storage_id}/{object_key}`。
    fn storage_root(&self, storage_id: &str) -> PathBuf {
        self.data_dir.join("storages").join(storage_id)
    }

    fn keys_or_err(&self) -> Result<&DerivedKeys, LibError> {
        self.keys.as_ref().ok_or(LibError::Locked)
    }

    // -----------------------------------------------------------------------
    // 会话
    // -----------------------------------------------------------------------

    /// 解锁：首次调用生成 KDF 参数（随机盐），派生 MK 与子密钥，载入索引。
    /// 口令与 MK 不落盘。
    pub fn unlock(&mut self, password: &str) -> Result<(), LibError> {
        let params_file = if self.kdf_params_path().exists() {
            let raw = fs::read(self.kdf_params_path())?;
            serde_json::from_slice::<KdfParamsFile>(&raw)
                .map_err(|e| LibError::Data(format!("kdf params corrupt: {e}")))?
        } else {
            let mut salt = [0u8; 16];
            getrandom::getrandom(&mut salt).map_err(|e| LibError::Data(format!("rng: {e}")))?;
            let p = KdfParams::standard();
            let f = KdfParamsFile {
                salt_hex: hex::encode(salt),
                m_cost_kib: p.m_cost_kib,
                t_cost: p.t_cost,
                parallelism: p.parallelism,
            };
            fs::create_dir_all(&self.data_dir)?;
            fs::write(self.kdf_params_path(), serde_json::to_vec_pretty(&f)?)?;
            f
        };
        let salt = hex::decode(&params_file.salt_hex)
            .map_err(|e| LibError::Data(format!("kdf salt corrupt: {e}")))?;
        let params = KdfParams {
            m_cost_kib: params_file.m_cost_kib,
            t_cost: params_file.t_cost,
            parallelism: params_file.parallelism,
        };
        let mk = derive_master_key(password.as_bytes(), &salt, &params)?;
        let master_salt = format!("heartshard-master-salt-v1:{}", params_file.salt_hex);
        self.keys = Some(DerivedKeys::new(mk, master_salt.as_bytes())?);
        Ok(())
    }

    pub fn lock(&mut self) {
        self.keys = None; // Zeroizing Drop 清零
    }

    pub fn is_unlocked(&self) -> bool {
        self.keys.is_some()
    }

    // -----------------------------------------------------------------------
    // 索引（M1：加密 JSON）
    // -----------------------------------------------------------------------

    fn load_index(&self) -> Result<LocalIndex, LibError> {
        let keys = self.keys_or_err()?;
        if !self.index_path().exists() {
            return Ok(LocalIndex::default());
        }
        let enc = fs::read(self.index_path())?;
        let json = decrypt_bare(keys.manifest_key.as_ref(), &enc)
            .map_err(|_| LibError::Data("local index decrypt failed (wrong password?)".into()))?;
        serde_json::from_slice(&json)
            .map_err(|e| LibError::Data(format!("local index corrupt: {e}")))
    }

    fn save_index(&self, index: &LocalIndex) -> Result<(), LibError> {
        let keys = self.keys_or_err()?;
        if let Some(parent) = self.index_path().parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec(index)?;
        let iv = generate_iv()?;
        let enc = encrypt_bare(keys.manifest_key.as_ref(), &iv, &json)?;
        // 原子写：临时文件 + rename
        let tmp = self.index_path().with_extension("tmp");
        fs::write(&tmp, &enc)?;
        fs::rename(&tmp, self.index_path())?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 模拟 Storage（M2 换成 BlobStore trait + S3/WebDAV 适配器）
    // -----------------------------------------------------------------------

    fn storage_put(&self, storage_id: &str, remote_key: &str, data: &[u8]) -> Result<(), LibError> {
        let path = self.remote_path(storage_id, remote_key)?;
        fs::create_dir_all(path.parent().ok_or_else(|| LibError::Data("bad key".into()))?)?;
        let tmp = path.with_extension("part");
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn storage_get(&self, storage_id: &str, remote_key: &str) -> Option<Vec<u8>> {
        let path = self.remote_path(storage_id, remote_key).ok()?;
        fs::read(path).ok()
    }

    fn remote_path(&self, storage_id: &str, remote_key: &str) -> Result<PathBuf, LibError> {
        let root = self.storage_root(storage_id);
        let mut p = root;
        for seg in remote_key.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." {
                return Err(LibError::Data(format!("illegal remote key: {remote_key}")));
            }
            p.push(seg);
        }
        Ok(p)
    }

    // -----------------------------------------------------------------------
    // 导入 / 列表 / 导出 / 校验
    // -----------------------------------------------------------------------

    /// 导入单个文件：SHA-256 去重（§8.4）→ encode_blob → shard 落盘 →
    /// 加密 manifest 落盘 → 写索引。M1 整文件读入内存（大文件流式在 M1.5）。
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
        let meta = fs::metadata(path)?;
        let taken_at = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let blob = fs::read(path)?;
        let blob_sha256 = crypto::sha256(&blob);

        let mut index = self.load_index()?;
        if index.blobs.iter().any(|b| b.blob_hash == hex::encode(blob_sha256)) {
            return Ok((hex::encode(blob_sha256), true)); // 去重：只跳过上传
        }

        let blob_key = keys.blob_key(&blob_sha256)?;
        let encoded = encode_blob(&blob, &blob_key, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M, generate_iv)?;
        let blob_hex = hex::encode(encoded.blob_sha256);

        // shard 落盘（模拟上传到 Storage）
        let mut manifest_shards = Vec::with_capacity(encoded.shards.len());
        for s in &encoded.shards {
            self.storage_put(MOCK_STORAGE_ID, &s.object_key, &s.data)?;
            manifest_shards.push(ManifestShard {
                chunk_index: s.chunk_index,
                shard_index: s.shard_index,
                storage_id: MOCK_STORAGE_ID.to_string(),
                remote_key: s.object_key.clone(),
                size: s.data.len() as u32,
                sha256: hex::encode(s.sha256),
            });
        }

        // 加密 manifest 落盘（§6.4：M2 起冗余 ≥2 Storage；M1 单存储单副本，见已知限制）
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
        self.storage_put(MOCK_STORAGE_ID, &manifest_key_path, &enc_manifest)?;

        // 写索引
        index.blobs.push(BlobEntry {
            blob_hash: blob_hex.clone(),
            file_name: manifest.file_name.clone(),
            mime: manifest.mime.clone(),
            total_len: manifest.total_len,
            taken_at,
            imported_at: now_secs(),
        });
        self.save_index(&index)?;
        Ok((blob_hex, false))
    }

    pub fn list_blobs(&self) -> Result<Vec<BlobEntry>, LibError> {
        let mut index = self.load_index()?;
        index.blobs.sort_by_key(|b| std::cmp::Reverse(b.taken_at));
        Ok(index.blobs)
    }

    /// 导出还原：读加密 manifest → fetch 闭包拉 shard → RS 重组 → 解密 →
    /// per-chunk 与 blob 级 SHA-256 校验 → 写目标文件。
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

        let storage_id = manifest.shards.first().map(|s| s.storage_id.clone()).unwrap_or_default();
        let decoded = shard::decode_blob(
            &blob_hash_bytes,
            &blob_key,
            &chunk_infos,
            manifest.n,
            manifest.m,
            |chunk_index, shard_index| {
                manifest
                    .shards
                    .iter()
                    .find(|s| s.chunk_index == chunk_index && s.shard_index == shard_index)
                    .and_then(|s| self.storage_get(&storage_id, &s.remote_key))
            },
        )?;

        // blob 级 SHA-256 校验（decode_blob 已做 per-chunk 校验）
        let final_hash = crypto::sha256(&decoded);
        if final_hash != blob_hash_bytes {
            return Err(LibError::Data(format!(
                "blob sha256 mismatch after decode: {}",
                blob_hex
            )));
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

    /// 健康检查：manifest 存在 + 每个远端 shard 存在且 SHA-256 一致。
    pub fn verify_blob(&self, blob_hash: &str) -> Result<VerifyReport, LibError> {
        let blob_hex = blob_hash.to_lowercase();
        let manifest = self.load_manifest(&blob_hex)?;
        let mut missing = Vec::new();
        let mut corrupted = Vec::new();
        let storage_id = manifest.shards.first().map(|s| s.storage_id.clone()).unwrap_or_default();
        for s in &manifest.shards {
            match self.storage_get(&storage_id, &s.remote_key) {
                None => missing.push(s.remote_key.clone()),
                Some(data) => {
                    if data.len() as u32 != s.size
                        || hex::encode(crypto::sha256(&data)) != s.sha256
                    {
                        corrupted.push(s.remote_key.clone());
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

    fn load_manifest(&self, blob_hex: &str) -> Result<Manifest, LibError> {
        let keys = self.keys_or_err()?;
        let remote_key = manifest_object_key(blob_hex)?;
        let enc = self
            .storage_get(MOCK_STORAGE_ID, &remote_key)
            .ok_or_else(|| LibError::ManifestMissing(blob_hex.to_string()))?;
        let json = decrypt_bare(keys.manifest_key.as_ref(), &enc)
            .map_err(|_| LibError::ManifestMissing(blob_hex.to_string()))?;
        serde_json::from_slice(&json).map_err(|e| LibError::Data(format!("manifest corrupt: {e}")))
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

/// M1 用扩展名推断 MIME（EXIF/容器探测在 M1.5）。
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
