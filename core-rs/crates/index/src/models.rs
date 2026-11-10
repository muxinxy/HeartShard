//! 跨设备同步实体（简报 §7.1 的 M2 子集，加同步版本列）。

use serde::{Deserialize, Serialize};

/// 带同步版本的记录。合并规则见 [`crate::merge`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Versioned<T> {
    /// 记录最后变更时所在设备的 Lamport 时钟值。
    pub lamport: u64,
    /// 最后变更的设备 id。
    pub device_id: String,
    pub record: T,
}

/// blob 元数据。`hash` 是全局主键（64 hex 小写）。
/// `file_name` 为时间线展示用（简报 §7.1 schema 的超集列，随 SQLCipher 加密落盘）。
/// `gps_lat`/`gps_lon` 为 EXIF GPS（§10 地图视图；schema 超集列）。f64 无 Eq。
/// `favorite` 为收藏标记（相册应用核心交互；schema 超集列）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobRecord {
    pub hash: String,
    pub file_name: String,
    pub size: i64,
    pub mime: String,
    pub width: i64,
    pub height: i64,
    pub taken_at: i64,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub chunk_size: i64,
    pub n: i64,
    pub m: i64,
    /// 加密 manifest 的对象键（四端同构，keypath 生成）。
    pub manifest_storage_key: String,
    pub created_at: i64,
    /// 删除墓碑（unix 秒）；Some 即已删除。
    pub deleted_at: Option<i64>,
    /// 收藏（♥）；旧设备同步来的 JSON 无此字段时默认 false。
    #[serde(default)]
    pub favorite: bool,
    /// 动态照片主图（Android motion photo / Apple Live Photo 配对主图）。
    #[serde(default)]
    pub is_motion: bool,
    /// 配对视频段的 blob hash（动态照片）；非动态照片为空串。
    #[serde(default)]
    pub motion_video_hash: String,
}

/// 相册。`id` 为生成 uuid（字符串），跨设备稳定。
/// `cover_hash` 为封面 blob（§10 封面设置；schema 超集列）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRecord {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
    /// 目录导入来源路径——`local_only`，不同步（简报 §8.3）。
    pub import_source: Option<String>,
    pub local_only: bool,
    pub cover_hash: String,
    pub created_at: i64,
    pub deleted_at: Option<i64>,
}

/// blob ↔ 相册关联。主键 `(blob_hash, album_id)`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobAlbumRecord {
    pub blob_hash: String,
    pub album_id: String,
    pub added_at: i64,
    pub deleted_at: Option<i64>,
}
