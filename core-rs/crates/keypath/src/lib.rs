//! HeartShard 对象键生成与文件名校验。
//!
//! 本 crate 是四端（Windows/Android/iOS/鸿蒙）唯一的键路径实现：
//! 壳层不得各自实现 `sanitize_key_segment` 或对象键生成器，
//! 跨端一致性由 `tools/keypath-tests/vectors.json` 共享向量锁定。
//!
//! 规范见 `docs/keypath-spec.md`（§5.1/§5.2 of the project brief）：
//! - shard 键：`blobs/{hash[0:2]}/{hash[2:4]}/{blobHash}/{chunkIndex:05d}_{shardIndex:02d}.shard`
//! - manifest 键：`manifests/{hash[0:2]}/{hash[2:4]}/{blobHash}/manifest.bin`
//! - 索引键：`index/{deviceId}/{lamportClock}.idx`
//! - 文件名只允许 `[a-z0-9._-]`，禁止 Windows 保留名及其子串。

use thiserror::Error;

/// 对象键/文件名校验错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum KeypathError {
    #[error("key segment must not be empty")]
    Empty,
    #[error("key segment exceeds {MAX_SEGMENT_BYTES} bytes (got {got})")]
    TooLong { got: usize },
    #[error("key segment contains forbidden character {0:?}")]
    ForbiddenChar(char),
    #[error("key segment must not start or end with '.'")]
    DotEdge,
    #[error("key segment must not contain a Windows reserved name (found {0:?})")]
    ReservedName(&'static str),
    #[error("invalid blob hash: expected 64 lowercase hex chars, got {0:?}")]
    InvalidBlobHash(String),
    #[error("generated key failed self-validation: {0}")]
    GeneratedKeyInvalid(String),
}

/// 单段文件名/目录名的最大字节数（UTF-8）。
/// 对齐 NTFS 单组件 255 UTF-16 码元上限；生成的键远短于此。
pub const MAX_SEGMENT_BYTES: usize = 255;

/// Windows 保留名（不区分大小写）。按项目规范，**作为子串出现即禁止**。
/// 生成的键只含十六进制与固定字面量，不会命中；此规则防御未来手工键。
pub const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// 校验单段文件名/目录名。
///
/// 规则（docs/mixfile-study.md §5.2 硬约束）：
/// 1. 非空，且 ≤ 255 字节（UTF-8）；
/// 2. 仅允许小写 ASCII 字母、数字、`_`、`-`、`.`；
/// 3. 不得以 `.` 开头或结尾（排除 `.` / `..` 与 Windows 尾点截断问题）；
/// 4. 不得包含任何 Windows 保留名子串（不区分大小写）。
///
/// 注意：这是**校验**而非清洗——生成的键永远合法，非法输入应被视为程序错误。
/// 相册名等用户可控字符串永远不进入对象键（项目简报 §8.1），无需在此清洗。
pub fn sanitize_key_segment(segment: &str) -> Result<&str, KeypathError> {
    if segment.is_empty() {
        return Err(KeypathError::Empty);
    }
    let byte_len = segment.len();
    if byte_len > MAX_SEGMENT_BYTES {
        return Err(KeypathError::TooLong { got: byte_len });
    }
    for ch in segment.chars() {
        let ok = matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.');
        if !ok {
            return Err(KeypathError::ForbiddenChar(ch));
        }
    }
    if segment.starts_with('.') || segment.ends_with('.') {
        return Err(KeypathError::DotEdge);
    }
    // 保留名子串检查：输入已限定为 ASCII，逐字节小写比较即可。
    let lower = segment.as_bytes();
    for reserved in WINDOWS_RESERVED {
        let r = reserved.as_bytes();
        if lower.windows(r.len()).any(|w| w == r) {
            return Err(KeypathError::ReservedName(reserved));
        }
    }
    Ok(segment)
}

/// 校验 blob SHA-256：64 个小写十六进制字符。
pub fn validate_blob_hash(hash: &str) -> Result<&str, KeypathError> {
    if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(KeypathError::InvalidBlobHash(hash.to_string()));
    }
    Ok(hash)
}

/// shard 对象键：`blobs/{hash[0:2]}/{hash[2:4]}/{blobHash}/{chunkIndex:05d}_{shardIndex:02d}.shard`
pub fn shard_object_key(
    blob_hash: &str,
    chunk_index: u32,
    shard_index: u8,
) -> Result<String, KeypathError> {
    validate_blob_hash(blob_hash)?;
    let key = format!(
        "blobs/{}/{}/{}/{:05}_{:02}.shard",
        &blob_hash[0..2],
        &blob_hash[2..4],
        blob_hash,
        chunk_index,
        shard_index
    );
    validate_generated(&key)?;
    Ok(key)
}

/// manifest 对象键：`manifests/{hash[0:2]}/{hash[2:4]}/{blobHash}/manifest.bin`
pub fn manifest_object_key(blob_hash: &str) -> Result<String, KeypathError> {
    validate_blob_hash(blob_hash)?;
    let key = format!(
        "manifests/{}/{}/{}/manifest.bin",
        &blob_hash[0..2],
        &blob_hash[2..4],
        blob_hash
    );
    validate_generated(&key)?;
    Ok(key)
}

/// 远端索引对象键：`index/{deviceId}/{lamportClock}.idx`
pub fn index_object_key(device_id: &str, lamport_clock: u64) -> Result<String, KeypathError> {
    sanitize_key_segment(device_id)?;
    let key = format!("index/{}/{}.idx", device_id, lamport_clock);
    validate_generated(&key)?;
    Ok(key)
}

/// 对生成的完整对象键做逐段自校验（防生成器自身回归）。
fn validate_generated(key: &str) -> Result<(), KeypathError> {
    for seg in key.split('/') {
        sanitize_key_segment(seg).map_err(|e| KeypathError::GeneratedKeyInvalid(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
