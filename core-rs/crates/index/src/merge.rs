//! LWW（Last-Writer-Wins）合并引擎——纯函数，可独立测试。
//!
//! 规则（简报 §7.2，两个裁决点已在 docs/mixfile-study.md 固化）：
//! 1. `lamport` 较大者胜；
//! 2. 同钟按 `device_id` **字典序较小者**胜（方向是本实现的确定性约定）；
//! 3. 完全同钟同设备（同一记录的两份拷贝）等价，保留任意一份。
//!
//! 收敛性：LWW-Register per key + 全序 (lamport, device_id) + 幂等合并 ⇒
//! 多设备以任意顺序应用同一批远端对象必达同一状态。

use crate::models::{BlobAlbumRecord, AlbumRecord, BlobRecord, Versioned};

/// 单条记录合并裁决结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    KeptLocal,
    TookRemote,
}

/// 胜负判定：`lamport` 大者胜；同钟按 `device_id` **字典序较小者**胜；
/// 完全相同（同钟同设备）视为等价，保留本地。
fn remote_wins<T>(local: &Versioned<T>, remote: &Versioned<T>) -> bool {
    if remote.lamport != local.lamport {
        return remote.lamport > local.lamport;
    }
    remote.device_id < local.device_id
}

/// 合并单条 blob 记录。
pub fn merge_blob(
    local: &Versioned<BlobRecord>,
    remote: &Versioned<BlobRecord>,
) -> MergeOutcome {
    if remote_wins(local, remote) {
        MergeOutcome::TookRemote
    } else {
        MergeOutcome::KeptLocal
    }
}

/// 合并单条相册记录。
pub fn merge_album(
    local: &Versioned<AlbumRecord>,
    remote: &Versioned<AlbumRecord>,
) -> MergeOutcome {
    if remote_wins(local, remote) {
        MergeOutcome::TookRemote
    } else {
        MergeOutcome::KeptLocal
    }
}

/// 合并单条关联记录。
pub fn merge_blob_album(
    local: &Versioned<BlobAlbumRecord>,
    remote: &Versioned<BlobAlbumRecord>,
) -> MergeOutcome {
    if remote_wins(local, remote) {
        MergeOutcome::TookRemote
    } else {
        MergeOutcome::KeptLocal
    }
}

/// Lamport 时钟推进：本地每次发起变更前 +1；收到远端对象后取 max。
pub fn fold_lamport(local: u64, seen: u64) -> u64 {
    local.max(seen)
}

#[cfg(test)]
mod tests;
