//! HeartShard 加密索引与多设备同步核心。
//!
//! - `models`：跨设备同步的实体（blobs / albums / blob_albums），每条带
//!   `(lamport, device_id)` 版本对。
//! - `merge`：LWW 合并引擎（简报 §7.2：较新 lamport 胜；同钟按 device_id
//!   字典序决胜——本实现取**字典序较小者胜**，确定性保证收敛）。
//! - `delta`：远端索引对象编解码（`index/{deviceId}/{lamport}.idx`，
//!   MANIFEST_KEY 加密）。
//! - `store`：BlobStore trait（put/get/list/delete）+ 本地目录实现（模拟 Storage）。
//! - `sqlite`：SQLCipher 本地索引（简报 §7.1 schema，加同步版本列）。
//! - `sync`：全量状态发布 + 拉取合并的同步引擎（M2；增量 delta 是后续优化）。
//!
//! 设计说明：
//! - blobs 自增 `id` 是设备本地的，**绝不参与同步**；跨设备关联一律用
//!   `hash`（blob）与 uuid（album）。
//! - `albums.import_source` / `local_only` 按 §8.3 不发布到远端。

pub mod delta;
pub mod merge;
pub mod models;
pub mod sqlite;
pub mod store;
pub mod sync;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("crypto: {0}")]
    Crypto(#[from] crypto::CryptoError),
    #[error("keypath: {0}")]
    Keypath(#[from] keypath::KeypathError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("store: {0}")]
    Store(#[from] store::StoreError),
    #[error("data: {0}")]
    Data(String),
}
