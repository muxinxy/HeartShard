//! SQLCipher 本地索引（简报 §7.1 schema + 同步版本列）。
//!
//! 加密：`bundled-sqlcipher`，密钥为 `index_key`（HKDF 从 MK 派生，
//! crypto::DerivedKeys）。打开后立即用 `SELECT count(*) FROM sqlite_master`
//! 验证密钥（错误口令在首次实际访问时报错）。

use rusqlite::{params, Connection, OptionalExtension};

use crate::merge::{merge_blob, merge_blob_album, merge_album, MergeOutcome};
use crate::models::{BlobAlbumRecord, AlbumRecord, BlobRecord, Versioned};
use crate::IndexError;

pub struct IndexDb {
    conn: Connection,
}

/// §7.1 schema（+ `sync_lamport`/`sync_device` 版本列与 `sync_applied` 表——
/// 后两者是同步引擎的实现列，简报 schema 的超集，向后兼容）。
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS blobs(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  hash TEXT UNIQUE NOT NULL,
  file_name TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL,
  mime TEXT NOT NULL DEFAULT '',
  width INTEGER NOT NULL DEFAULT 0,
  height INTEGER NOT NULL DEFAULT 0,
  taken_at INTEGER NOT NULL DEFAULT 0,
  chunk_size INTEGER NOT NULL,
  n INTEGER NOT NULL,
  m INTEGER NOT NULL,
  manifest_storage_key TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER,
  sync_lamport INTEGER NOT NULL DEFAULT 0,
  sync_device TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS shards(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  blob_id INTEGER NOT NULL REFERENCES blobs(id),
  chunk_index INTEGER NOT NULL,
  shard_index INTEGER NOT NULL,
  storage_id TEXT NOT NULL,
  remote_key TEXT NOT NULL,
  size INTEGER NOT NULL,
  etag TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  last_verified_at INTEGER,
  UNIQUE(blob_id, chunk_index, shard_index, storage_id)
);
CREATE TABLE IF NOT EXISTS albums(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  parent_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  import_source TEXT,
  local_only INTEGER NOT NULL DEFAULT 0,
  cover_hash TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL DEFAULT 0,
  deleted_at INTEGER,
  sync_lamport INTEGER NOT NULL DEFAULT 0,
  sync_device TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS blob_albums(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  blob_hash TEXT NOT NULL REFERENCES blobs(hash),
  album_id TEXT NOT NULL REFERENCES albums(id),
  added_at INTEGER NOT NULL,
  deleted_at INTEGER,
  sync_lamport INTEGER NOT NULL DEFAULT 0,
  sync_device TEXT NOT NULL DEFAULT '',
  UNIQUE(blob_hash, album_id)
);
CREATE TABLE IF NOT EXISTS tags(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT UNIQUE NOT NULL
);
CREATE TABLE IF NOT EXISTS blob_tags(
  blob_id INTEGER NOT NULL REFERENCES blobs(id),
  tag_id INTEGER NOT NULL REFERENCES tags(id),
  UNIQUE(blob_id, tag_id)
);
CREATE TABLE IF NOT EXISTS storages(
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  name TEXT NOT NULL,
  config_encrypted BLOB NOT NULL,
  weight INTEGER NOT NULL DEFAULT 1,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_health_at INTEGER,
  last_health_ok INTEGER
);
CREATE TABLE IF NOT EXISTS sync_state(
  device_id TEXT NOT NULL,
  lamport_clock INTEGER NOT NULL DEFAULT 0,
  last_pull_at INTEGER NOT NULL DEFAULT 0,
  last_published INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sync_applied(
  device_id TEXT NOT NULL,
  lamport INTEGER NOT NULL,
  applied_at INTEGER NOT NULL,
  PRIMARY KEY(device_id, lamport)
);
CREATE TABLE IF NOT EXISTS import_staging(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  abs_path TEXT NOT NULL,
  rel_path TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_blobs_taken_at ON blobs(taken_at);
CREATE INDEX IF NOT EXISTS idx_shards_blob ON shards(blob_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_staging_session_path
  ON import_staging(session_id, abs_path);
";

impl IndexDb {
    /// 打开（或创建）加密索引。`index_key` 来自 HKDF，不落盘。
    pub fn open(path: impl AsRef<Path>, index_key: &str) -> Result<Self, IndexError> {
        let conn = Connection::open(path.as_ref())?;
        conn.pragma_update(None, "key", index_key)?;
        // 用 sqlite_master 访问验证密钥；错钥时 SQLCipher 在此处报
        // "file is not a database"
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))?;
        let db = Self { conn };
        db.conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;",
        )?;
        Ok(db)
    }

    pub fn init_schema(&self) -> Result<(), IndexError> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    // ---------------- sync_state ----------------

    pub fn set_device_id(&self, device_id: &str) -> Result<(), IndexError> {
        let exists: Option<i64> = self
            .conn
            .query_row("SELECT 1 FROM sync_state LIMIT 1", [], |r| r.get(0))
            .optional()?;
        if exists.is_some() {
            self.conn.execute(
                "UPDATE sync_state SET device_id = ?1",
                params![device_id],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO sync_state(device_id, lamport_clock) VALUES (?1, 0)",
                params![device_id],
            )?;
        }
        Ok(())
    }

    pub fn device_id(&self) -> Result<String, IndexError> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT device_id FROM sync_state LIMIT 1", [], |r| r.get(0))
            .optional()?;
        v.ok_or_else(|| IndexError::Data("device id not initialized".into()))
    }

    pub fn lamport(&self) -> Result<u64, IndexError> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT lamport_clock FROM sync_state LIMIT 1", [], |r| r.get(0))
            .optional()?;
        Ok(v.unwrap_or(0).max(0) as u64)
    }

    pub fn set_lamport(&self, value: u64) -> Result<(), IndexError> {
        self.conn.execute(
            "UPDATE sync_state SET lamport_clock = ?1",
            params![value as i64],
        )?;
        Ok(())
    }

    /// 增量发布游标（§7 delta）：该 lamport 之前的记录已全量发布过。
    pub fn last_published(&self) -> Result<u64, IndexError> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT last_published FROM sync_state LIMIT 1", [], |r| r.get(0))
            .optional()?;
        Ok(v.unwrap_or(0).max(0) as u64)
    }

    pub fn set_last_published(&self, value: u64) -> Result<(), IndexError> {
        self.conn.execute(
            "UPDATE sync_state SET last_published = ?1",
            params![value as i64],
        )?;
        Ok(())
    }

    /// §7 delta：sync_lamport > since 的记录（增量发布用）。
    pub fn blobs_changed_since(&self, since: u64) -> Result<Vec<Versioned<BlobRecord>>, IndexError> {
        Ok(self
            .load_blobs()?
            .into_iter()
            .filter(|v| v.lamport > since)
            .collect())
    }

    pub fn albums_changed_since(&self, since: u64) -> Result<Vec<Versioned<AlbumRecord>>, IndexError> {
        Ok(self
            .load_albums()?
            .into_iter()
            .filter(|v| v.lamport > since)
            .collect())
    }

    pub fn blob_albums_changed_since(
        &self,
        since: u64,
    ) -> Result<Vec<Versioned<BlobAlbumRecord>>, IndexError> {
        Ok(self
            .load_blob_albums()?
            .into_iter()
            .filter(|v| v.lamport > since)
            .collect())
    }

    pub fn mark_applied(&self, device_id: &str, lamport: u64, at: i64) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO sync_applied(device_id, lamport, applied_at) VALUES (?1, ?2, ?3)",
            params![device_id, lamport as i64, at],
        )?;
        Ok(())
    }

    pub fn applied_contains(&self, device_id: &str, lamport: u64) -> Result<bool, IndexError> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM sync_applied WHERE device_id = ?1 AND lamport = ?2",
                params![device_id, lamport as i64],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.is_some())
    }

    // ---------------- blobs ----------------

    pub fn upsert_blob(&self, v: &Versioned<BlobRecord>) -> Result<MergeOutcome, IndexError> {
        let r = &v.record;
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT sync_lamport, sync_device FROM blobs WHERE hash = ?1",
                params![r.hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let local = existing.map(|(lamport, device)| Versioned {
            lamport: lamport.max(0) as u64,
            device_id: device,
            record: r.clone(),
        });
        let outcome = match &local {
            None => MergeOutcome::TookRemote,
            Some(local) => merge_blob(local, v),
        };
        if outcome == MergeOutcome::TookRemote {
            self.conn.execute(
                "INSERT INTO blobs(hash,file_name,size,mime,width,height,taken_at,chunk_size,n,m,manifest_storage_key,created_at,deleted_at,sync_lamport,sync_device)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
                 ON CONFLICT(hash) DO UPDATE SET
                   file_name=?2, size=?3, mime=?4, width=?5, height=?6, taken_at=?7, chunk_size=?8,
                   n=?9, m=?10, manifest_storage_key=?11, created_at=?12, deleted_at=?13,
                   sync_lamport=?14, sync_device=?15",
                params![
                    r.hash,
                    r.file_name,
                    r.size,
                    r.mime,
                    r.width,
                    r.height,
                    r.taken_at,
                    r.chunk_size,
                    r.n,
                    r.m,
                    r.manifest_storage_key,
                    r.created_at,
                    r.deleted_at,
                    v.lamport as i64,
                    v.device_id,
                ],
            )?;
        }
        Ok(outcome)
    }

    pub fn load_blobs(&self) -> Result<Vec<Versioned<BlobRecord>>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT hash,file_name,size,mime,width,height,taken_at,chunk_size,n,m,manifest_storage_key,created_at,deleted_at,sync_lamport,sync_device FROM blobs",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Versioned {
                lamport: row.get::<_, i64>(13)?.max(0) as u64,
                device_id: row.get(14)?,
                record: BlobRecord {
                    hash: row.get(0)?,
                    file_name: row.get(1)?,
                    size: row.get(2)?,
                    mime: row.get(3)?,
                    width: row.get(4)?,
                    height: row.get(5)?,
                    taken_at: row.get(6)?,
                    chunk_size: row.get(7)?,
                    n: row.get(8)?,
                    m: row.get(9)?,
                    manifest_storage_key: row.get(10)?,
                    created_at: row.get(11)?,
                    deleted_at: row.get(12)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ---------------- albums ----------------

    pub fn upsert_album(&self, v: &Versioned<AlbumRecord>) -> Result<MergeOutcome, IndexError> {
        let r = &v.record;
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT sync_lamport, sync_device FROM albums WHERE id = ?1",
                params![r.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let outcome = match existing {
            None => MergeOutcome::TookRemote,
            Some((lamport, device)) => merge_album(
                &Versioned {
                    lamport: lamport.max(0) as u64,
                    device_id: device,
                    record: r.clone(),
                },
                v,
            ),
        };
        if outcome == MergeOutcome::TookRemote {
            self.conn.execute(
                "INSERT INTO albums(id,name,parent_id,sort_order,import_source,local_only,cover_hash,created_at,deleted_at,sync_lamport,sync_device)
                 VALUES(?1,?2,?3,?4,?5,?6,?11,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET
                   name=?2, parent_id=?3, sort_order=?4, import_source=?5, local_only=?6,
                   cover_hash=?11, created_at=?7, deleted_at=?8, sync_lamport=?9, sync_device=?10",
                params![
                    r.id,
                    r.name,
                    r.parent_id,
                    r.sort_order,
                    r.import_source,
                    r.local_only as i64,
                    r.created_at,
                    r.deleted_at,
                    v.lamport as i64,
                    v.device_id,
                    r.cover_hash,
                ],
            )?;
        }
        Ok(outcome)
    }

    /// §10 封面设置：更新相册 cover_hash（同步列 lamport 推进）。
    pub fn album_set_cover(
        &self,
        album_id: &str,
        cover_hash: &str,
        lamport: u64,
        device_id: &str,
    ) -> Result<(), IndexError> {
        self.conn.execute(
            "UPDATE albums SET cover_hash = ?2, sync_lamport = ?3, sync_device = ?4 WHERE id = ?1",
            params![album_id, cover_hash, lamport as i64, device_id],
        )?;
        Ok(())
    }

    /// §10 拖拽排序/移动：更新 sort_order 与 parent_id。
    pub fn album_set_sort(
        &self,
        album_id: &str,
        sort_order: i64,
        lamport: u64,
        device_id: &str,
    ) -> Result<(), IndexError> {
        self.conn.execute(
            "UPDATE albums SET sort_order = ?2, sync_lamport = ?3, sync_device = ?4 WHERE id = ?1",
            params![album_id, sort_order, lamport as i64, device_id],
        )?;
        Ok(())
    }

    /// §10 相册树数据：未删除相册（含封面 hash）。
    pub fn load_albums(&self) -> Result<Vec<Versioned<AlbumRecord>>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id,name,parent_id,sort_order,import_source,local_only,cover_hash,created_at,deleted_at,sync_lamport,sync_device FROM albums",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Versioned {
                lamport: row.get::<_, i64>(9)?.max(0) as u64,
                device_id: row.get(10)?,
                record: AlbumRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    sort_order: row.get(3)?,
                    import_source: row.get(4)?,
                    local_only: row.get::<_, i64>(5)? != 0,
                    cover_hash: row.get(6)?,
                    created_at: row.get(7)?,
                    deleted_at: row.get(8)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 相册内 blob 计数 + 封面候选（§10 相册树 UI）。
    pub fn album_stats(&self) -> Result<Vec<(String, i64, Option<String>)>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT ba.album_id, COUNT(*), MIN(ba.blob_hash)
             FROM blob_albums ba
             WHERE ba.deleted_at IS NULL
             GROUP BY ba.album_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ---------------- blob_albums ----------------

    pub fn upsert_blob_album(
        &self,
        v: &Versioned<BlobAlbumRecord>,
    ) -> Result<MergeOutcome, IndexError> {
        let r = &v.record;
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT sync_lamport, sync_device FROM blob_albums WHERE blob_hash = ?1 AND album_id = ?2",
                params![r.blob_hash, r.album_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let outcome = match existing {
            None => MergeOutcome::TookRemote,
            Some((lamport, device)) => merge_blob_album(
                &Versioned {
                    lamport: lamport.max(0) as u64,
                    device_id: device,
                    record: r.clone(),
                },
                v,
            ),
        };
        if outcome == MergeOutcome::TookRemote {
            self.conn.execute(
                "INSERT INTO blob_albums(blob_hash,album_id,added_at,deleted_at,sync_lamport,sync_device)
                 VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(blob_hash, album_id) DO UPDATE SET
                   added_at=?3, deleted_at=?4, sync_lamport=?5, sync_device=?6",
                params![
                    r.blob_hash,
                    r.album_id,
                    r.added_at,
                    r.deleted_at,
                    v.lamport as i64,
                    v.device_id,
                ],
            )?;
        }
        Ok(outcome)
    }

    pub fn load_blob_albums(&self) -> Result<Vec<Versioned<BlobAlbumRecord>>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT blob_hash,album_id,added_at,deleted_at,sync_lamport,sync_device FROM blob_albums",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Versioned {
                lamport: row.get::<_, i64>(4)?.max(0) as u64,
                device_id: row.get(5)?,
                record: BlobAlbumRecord {
                    blob_hash: row.get(0)?,
                    album_id: row.get(1)?,
                    added_at: row.get(2)?,
                    deleted_at: row.get(3)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ---------------- import_staging（§8.5 五阶段向导续传） ----------------

    /// 写入/更新一条 staging 记录（同 session+abs_path 幂等）。
    pub fn staging_upsert(
        &self,
        session_id: &str,
        abs_path: &str,
        rel_path: &str,
        size: i64,
        mtime: i64,
        sha256_hex: &str,
    ) -> Result<i64, IndexError> {
        self.conn.execute(
            "INSERT INTO import_staging(session_id,abs_path,rel_path,size,mtime,sha256,status)
             VALUES(?1,?2,?3,?4,?5,?6,'pending')
             ON CONFLICT(session_id, abs_path) DO UPDATE SET
               rel_path=?3, size=?4, mtime=?5, sha256=?6",
            params![session_id, abs_path, rel_path, size, mtime, sha256_hex],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 按 status 更新 staging 行。
    pub fn staging_set_status(
        &self,
        id: i64,
        status: &str,
        error: &str,
    ) -> Result<(), IndexError> {
        self.conn.execute(
            "UPDATE import_staging SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, status, error],
        )?;
        Ok(())
    }

    pub fn staging_load(
        &self,
        session_id: &str,
    ) -> Result<Vec<StagingRow>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id,abs_path,rel_path,size,mtime,sha256,status,error FROM import_staging
             WHERE session_id = ?1 ORDER BY rel_path",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(StagingRow {
                id: row.get(0)?,
                abs_path: row.get(1)?,
                rel_path: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                sha256: row.get(5)?,
                status: row.get(6)?,
                error: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn staging_clear(&self, session_id: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "DELETE FROM import_staging WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// 按 import_source 找既有相册（§8.3 幂等建相册）。
    pub fn album_by_import_source(
        &self,
        import_source: &str,
    ) -> Result<Option<Versioned<AlbumRecord>>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id,name,parent_id,sort_order,import_source,local_only,cover_hash,created_at,deleted_at,sync_lamport,sync_device
             FROM albums WHERE import_source = ?1 LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![import_source], |row| {
                Ok(Versioned {
                    lamport: row.get::<_, i64>(9)?.max(0) as u64,
                    device_id: row.get(10)?,
                    record: AlbumRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        sort_order: row.get(3)?,
                        import_source: row.get(4)?,
                        local_only: row.get::<_, i64>(5)? != 0,
                        cover_hash: row.get(6)?,
                        created_at: row.get(7)?,
                        deleted_at: row.get(8)?,
                    },
                })
            })
            .optional()?;
        Ok(row)
    }
}

/// import_staging 行（含新增的 error 列——schema 超集，见模块注释）。
#[derive(Debug, Clone)]
pub struct StagingRow {
    pub id: i64,
    pub abs_path: String,
    pub rel_path: String,
    pub size: i64,
    pub mtime: i64,
    pub sha256: String,
    pub status: String,
    pub error: String,
}

use std::path::Path;
