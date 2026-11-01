//! HeartShard blob → chunk → 加密 chunk → RS shard 全管线。
//!
//! 流程（简报 §6）：
//! - 编码：blob 按 `chunk_size` 切块 → 每 chunk 随机 IV + `blob_key` AES-256-GCM 加密
//!   （AAD 绑定 blob_hash/chunk_index/total_chunks）→ RS(n,m) 分散为 n 个 shard →
//!   每个 shard 计算 SHA-256 并生成对象键（keypath）。
//! - 解码：按 manifest 的 per-chunk 信息从存储拉取 shard → RS 重组 → 解密（AAD 校验）→
//!   每 chunk 明文 SHA-256 校验 → 拼接 → blob 级 SHA-256 校验。
//!
//! 完整性链条：shard 损坏 → RS 重组后 GCM tag/AAD 失败 → 报错。
//! （剔除坏 shard 重试与修复属 M5 修复任务，本 crate 只暴露确定性原语。）

use crypto::{decrypt_chunk, encrypt_chunk, CryptoError, BLOB_HASH_LEN, IV_LEN};
use ida::{DispersalConfig, IdaError};
use keypath::{shard_object_key, KeypathError};
use thiserror::Error;

/// 全局默认分片大小：1 MiB。
pub const DEFAULT_CHUNK_SIZE: usize = 1 << 20;
/// 分片下限：256 KiB。
pub const MIN_CHUNK_SIZE: usize = 256 * 1024;
/// 分片上限：4 MiB（简报 §5.3；MixFile 的 20 MiB 上限不开放）。
pub const MAX_CHUNK_SIZE: usize = 4 << 20;
/// 默认总 shard 数。
pub const DEFAULT_N: u8 = 4;
/// 默认恢复阈值。
pub const DEFAULT_M: u8 = 2;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ShardError {
    #[error("chunk size must be between {MIN} and {MAX} bytes, got {got}", MIN = MIN_CHUNK_SIZE, MAX = MAX_CHUNK_SIZE)]
    InvalidChunkSize { got: usize },
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("ida: {0}")]
    Ida(#[from] IdaError),
    #[error("keypath: {0}")]
    Keypath(#[from] KeypathError),
    #[error("chunk {chunk_index}: got {got} shards, need {need}")]
    MissingShards {
        chunk_index: u32,
        got: usize,
        need: usize,
    },
    #[error("chunk {0}: shards present but cannot be decoded/decrypted (corruption or manifest mismatch)")]
    ChunkCorrupted(u32),
    #[error("blob SHA-256 mismatch after reassembly")]
    BlobIntegrity,
}

/// 校验分片大小（三层覆盖的最终权值都应经过此校验）。
pub fn validate_chunk_size(size: usize) -> Result<(), ShardError> {
    if (MIN_CHUNK_SIZE..=MAX_CHUNK_SIZE).contains(&size) {
        Ok(())
    } else {
        Err(ShardError::InvalidChunkSize { got: size })
    }
}

/// 单个加密 chunk 的编码产物（写入 manifest 的数据来源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedChunk {
    pub chunk_index: u32,
    /// chunk 明文 SHA-256（MixFile 语义保留，见 docs/mixfile-study.md §2.2#5）。
    pub plaintext_sha256: [u8; 32],
    /// 线上格式 `iv(12) ‖ ct ‖ tag(12)`。
    pub ciphertext: Vec<u8>,
}

/// 单个待上传 shard 对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardDescriptor {
    pub chunk_index: u32,
    pub shard_index: u8,
    pub data: Vec<u8>,
    pub sha256: [u8; 32],
    /// 对象键：`blobs/{hash[0:2]}/{hash[2:4]}/{blobHash}/{chunk:05}_{shard:02}.shard`
    pub object_key: String,
}

/// blob 编码全产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedBlob {
    pub blob_sha256: [u8; 32],
    pub total_len: usize,
    pub chunk_size: usize,
    pub n: u8,
    pub m: u8,
    pub chunks: Vec<EncodedChunk>,
    pub shards: Vec<ShardDescriptor>,
}

/// 编码整个 blob。
///
/// `iv_fn` 每次调用返回一个新 IV；生产环境传 `crypto::generate_iv`，
/// 测试可注入确定性 IV 序列。
pub fn encode_blob(
    blob: &[u8],
    blob_key: &[u8; 32],
    chunk_size: usize,
    n: u8,
    m: u8,
    mut iv_fn: impl FnMut() -> Result<[u8; IV_LEN], CryptoError>,
) -> Result<EncodedBlob, ShardError> {
    validate_chunk_size(chunk_size)?;
    let config = DispersalConfig::new(n, m)?;
    let blob_sha256 = crypto::sha256(blob);
    let total_chunks = blob.len().div_ceil(chunk_size) as u32;

    let mut chunks = Vec::new();
    let mut shards = Vec::new();
    for (chunk_index, chunk) in blob.chunks(chunk_size).enumerate() {
        let chunk_index = chunk_index as u32;
        let plaintext_sha256 = crypto::sha256(chunk);
        let iv = iv_fn()?;
        let ciphertext = encrypt_chunk(
            blob_key,
            &iv,
            &blob_sha256,
            chunk_index,
            total_chunks,
            chunk,
        )?;

        let parts = ida::encode_shards(&ciphertext, config)?;
        for (shard_index, data) in parts.into_iter().enumerate() {
            let sha256 = crypto::sha256(&data);
            let object_key =
                shard_object_key(&hex_lower(&blob_sha256), chunk_index, shard_index as u8)?;
            shards.push(ShardDescriptor {
                chunk_index,
                shard_index: shard_index as u8,
                data,
                sha256,
                object_key,
            });
        }
        chunks.push(EncodedChunk {
            chunk_index,
            plaintext_sha256,
            ciphertext,
        });
    }

    Ok(EncodedBlob {
        blob_sha256,
        total_len: blob.len(),
        chunk_size,
        n,
        m,
        chunks,
        shards,
    })
}

/// 解码所需的 per-chunk manifest 信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkManifestInfo {
    pub plaintext_sha256: [u8; 32],
    /// 线上密文长度（含 iv+tag），即该 chunk 的 RS 数据长度。
    pub ciphertext_len: usize,
}

/// 从 shard 拉取函数解码整个 blob。
///
/// `fetch(chunk_index, shard_index) -> Option<data>`：None 表示该 shard 不可用
/// （存储丢失/网络失败）。每个 chunk 可用 shard ≥ m 即可恢复。
/// `blob_hash` 参与解密 AAD 绑定（与编码侧 [`encode_blob`] 的 blob SHA-256 一致）。
pub fn decode_blob(
    blob_hash: &[u8; BLOB_HASH_LEN],
    blob_key: &[u8; 32],
    chunk_infos: &[ChunkManifestInfo],
    n: u8,
    m: u8,
    mut fetch: impl FnMut(u32, u8) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, ShardError> {
    let config = DispersalConfig::new(n, m)?;
    let total_chunks = chunk_infos.len() as u32;
    let mut blob = Vec::new();

    for (chunk_index, info) in chunk_infos.iter().enumerate() {
        let chunk_index = chunk_index as u32;
        let mut available: Vec<(u8, Vec<u8>)> = Vec::new();
        for shard_index in 0..n {
            if let Some(data) = fetch(chunk_index, shard_index) {
                available.push((shard_index, data));
            }
        }
        if available.len() < usize::from(m) {
            return Err(ShardError::MissingShards {
                chunk_index,
                got: available.len(),
                need: usize::from(m),
            });
        }
        let refs: Vec<(u8, &[u8])> =
            available.iter().map(|(i, d)| (*i, d.as_slice())).collect();
        let ciphertext = ida::decode_shards(&refs, config, info.ciphertext_len)
            .map_err(|_| ShardError::ChunkCorrupted(chunk_index))?;
        if ciphertext.len() != info.ciphertext_len {
            return Err(ShardError::ChunkCorrupted(chunk_index));
        }
        let plaintext = decrypt_chunk(blob_key, &ciphertext, blob_hash, chunk_index, total_chunks)
            .map_err(|_| ShardError::ChunkCorrupted(chunk_index))?;
        let pt_hash = crypto::sha256(&plaintext);
        if pt_hash != info.plaintext_sha256 {
            return Err(ShardError::ChunkCorrupted(chunk_index));
        }
        blob.extend_from_slice(&plaintext);
    }
    Ok(blob)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[usize::from(b >> 4)] as char);
        s.push(HEX[usize::from(b & 0x0f)] as char);
    }
    s
}

#[cfg(test)]
mod tests;
