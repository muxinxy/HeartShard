//! HeartShard 纠删码分散（IDA，Information Dispersal）。
//!
//! 语义（裁决 Q6，简报 §6.3）：`n` = 总 shard 数，`m` = 恢复阈值。
//! 实现：数据切成 `m` 个 data shard，RS 生成 `n - m` 个 recovery shard，
//! 共 `n` 个；任意 `m` 个（无论 data/recovery 混合）可恢复原文。
//! 存储放大 = `n / m`（默认 n=4, m=2 → 2×）。
//!
//! 底层用 `reed-solomon-simd`（Leopard-RS，GF(2^16)）。
//! shard 不能与经典 Vandermonde/Cauchy 实现（如 reed-solomon-erasure）互换，
//! manifest 必须记录 codec 标识（shard crate 负责）。
//!
//! RS 只纠删除、不纠位翻转：损坏 shard 依赖上层（shard crate）的
//! per-shard SHA-256 识别后剔除，再以剩余 shard 解码。

use thiserror::Error;

/// RS 引擎标识，写入 manifest 的 `codec` 字段。
pub const CODEC_ID: &str = "leopard-rs/reed-solomon-simd-3";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IdaError {
    #[error("invalid dispersal config: need 1 <= m <= n, got n={n}, m={m}")]
    InvalidConfig { n: u8, m: u8 },
    #[error("not enough shards to recover: got {got}, need {need}")]
    NotEnoughShards { got: usize, need: usize },
    #[error("shard {0} has wrong length (expected {1}, got {2})")]
    ShardLengthMismatch(usize, usize, usize),
    #[error("shard payload must have even length (got {0})")]
    OddShardLength(usize),
    #[error("RS decode failed: provided shards are inconsistent (wrong config or corrupted length set)")]
    DecodeFailed,
}

/// 分散配置：n 总 shard 数，m 恢复阈值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispersalConfig {
    pub n: u8,
    pub m: u8,
}

impl DispersalConfig {
    pub fn new(n: u8, m: u8) -> Result<Self, IdaError> {
        if m == 0 || n == 0 || m > n {
            return Err(IdaError::InvalidConfig { n, m });
        }
        Ok(Self { n, m })
    }

    /// data shard 数 = m，recovery shard 数 = n - m。
    pub fn data_shards(&self) -> usize {
        self.m as usize
    }

    pub fn recovery_shards(&self) -> usize {
        self.n as usize - self.m as usize
    }
}

/// 把 `data` 切成 `m` 个等长（偶数）data shard，末尾零填充。
/// 返回 (shards, 填充后总长)；原始长度由调用方记录（ct 长度在 manifest 中）。
fn split_into_data_shards(data: &[u8], k: usize) -> (Vec<Vec<u8>>, usize) {
    if data.is_empty() {
        return (vec![Vec::new(); k], 0);
    }
    // 每 shard 长度：ceil(len/k)，向上取偶（reed-solomon-simd 要求 shard 长度为偶数）
    let base = data.len().div_ceil(k);
    let shard_len = base.div_ceil(2) * 2;
    let mut shards = Vec::with_capacity(k);
    for i in 0..k {
        let start = i * shard_len;
        let end = std::cmp::min(start + shard_len, data.len());
        let mut shard = vec![0u8; shard_len];
        if start < data.len() {
            shard[..end - start].copy_from_slice(&data[start..end]);
        }
        shards.push(shard);
    }
    (shards, shard_len * k)
}

/// 把数据分散为 `n` 个 shard（前 `m` 个为 data shard，其余为 recovery shard）。
/// 空数据返回 `n` 个空 shard。
pub fn encode_shards(data: &[u8], config: DispersalConfig) -> Result<Vec<Vec<u8>>, IdaError> {
    let k = config.data_shards();
    if data.is_empty() {
        return Ok(vec![Vec::new(); config.n as usize]);
    }
    let (originals, _padded_len) = split_into_data_shards(data, k);
    if config.recovery_shards() == 0 {
        // n == m：无冗余配置，Leopard 不接受 0 个 recovery shard，直接返回 data shards
        return Ok(originals);
    }
    let recovery = reed_solomon_simd::encode(k, config.recovery_shards(), &originals)
        .map_err(|_| IdaError::DecodeFailed)?;
    let mut out = originals;
    out.extend(recovery);
    Ok(out)
}

/// 从 `shards`（任意 `m` 个，`(shard_index, payload)`，index 0..n）恢复原始数据。
/// `data_len` 为原始数据长度（截除零填充）。
pub fn decode_shards(
    shards: &[(u8, &[u8])],
    config: DispersalConfig,
    data_len: usize,
) -> Result<Vec<u8>, IdaError> {
    if shards.len() < config.m as usize {
        return Err(IdaError::NotEnoughShards {
            got: shards.len(),
            need: config.m as usize,
        });
    }
    if data_len == 0 {
        return Ok(Vec::new());
    }
    let k = config.data_shards();
    let shard_len = shards[0].1.len();
    if shard_len == 0 || !shard_len.is_multiple_of(2) {
        return Err(IdaError::OddShardLength(shard_len));
    }

    let mut originals: Vec<Option<&[u8]>> = vec![None; k];
    let mut recoveries: Vec<(usize, &[u8])> = Vec::new();
    for &(idx, payload) in shards {
        if usize::from(idx) >= config.n as usize {
            return Err(IdaError::DecodeFailed);
        }
        if payload.len() != shard_len {
            return Err(IdaError::ShardLengthMismatch(
                usize::from(idx),
                shard_len,
                payload.len(),
            ));
        }
        if usize::from(idx) < k {
            originals[usize::from(idx)] = Some(payload);
        } else {
            recoveries.push((usize::from(idx) - k, payload));
        }
    }

    let missing = k - originals.iter().filter(|s| s.is_some()).count();
    if missing > recoveries.len() {
        return Err(IdaError::NotEnoughShards {
            got: shards.len(),
            need: config.m as usize,
        });
    }

    // 拼回：缺失的 data shard 由 RS 解码补齐（快路径：无缺失则跳过 RS 运算）
    let mut restored: std::collections::BTreeMap<usize, Vec<u8>> = if missing == 0 {
        std::collections::BTreeMap::new()
    } else {
        let original_with_idx: Vec<(usize, &[u8])> = originals
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|p| (i, p)))
            .collect();
        reed_solomon_simd::decode(k, config.recovery_shards(), original_with_idx, recoveries)
            .map_err(|_| IdaError::DecodeFailed)?
    };

    let mut out = Vec::with_capacity(data_len);
    for (i, part) in originals.iter().enumerate() {
        let owned;
        let payload: &[u8] = match part {
            Some(p) => p,
            None => {
                owned = restored.remove(&i).ok_or(IdaError::DecodeFailed)?;
                owned.as_slice()
            }
        };
        let take = std::cmp::min(payload.len(), data_len - out.len());
        out.extend_from_slice(&payload[..take]);
        if out.len() >= data_len {
            break;
        }
    }
    if out.len() != data_len {
        return Err(IdaError::DecodeFailed);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
