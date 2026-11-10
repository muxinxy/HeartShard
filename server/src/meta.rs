//! 元数据存储抽象（M5b''）：users/groups/acls/shares/sources 全部落数据库，
//! 不再使用 JSON 文件。驱动可切换：
//! - `sqlite`（默认）：rusqlite bundled，零配置，文件 `meta.db`
//! - `mysql`：mysql_async，`mysql://user:pass@host/db`（server.toml `database.url`）
//!
//! 记录结构沿用现有 User/Group/AclEntry/Share/SourceConfig（serde JSON 列，
//! 避免四套表结构 × 两种方言的维护成本；键查询都在主键列上）。

use crate::acl::AclEntry;
use crate::auth::{Group, User};
use crate::share::Share;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRow {
    pub album_id: String,
    pub principal_kind: String,
    pub principal_id: String,
    pub perm: String,
}

pub trait MetaStore: Send + Sync {
    fn list_users(&self) -> Result<Vec<User>, String>;
    fn save_user(&self, user: &User) -> Result<(), String>;
    fn delete_user(&self, uid: &str) -> Result<(), String>;

    fn list_groups(&self) -> Result<Vec<Group>, String>;
    fn save_groups(&self, groups: &[Group]) -> Result<(), String>;

    fn list_acls(&self) -> Result<Vec<AclEntry>, String>;
    fn upsert_acl(&self, entry: &AclEntry) -> Result<(), String>;
    fn delete_acl(&self, album_id: &str, kind: &str, id: &str) -> Result<(), String>;

    fn list_shares(&self) -> Result<Vec<Share>, String>;
    fn save_share(&self, share: &Share) -> Result<(), String>;
    fn delete_share(&self, id: &str) -> Result<(), String>;

    fn list_sources(&self) -> Result<Vec<serde_json::Value>, String>;
    fn save_sources(&self, sources: &[serde_json::Value]) -> Result<(), String>;
}

// ---------------- SQLite（默认） ----------------

pub struct SqliteMeta {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteMeta {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS meta_users(uid TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta_groups(gid TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta_acls(album_id TEXT NOT NULL, principal_kind TEXT NOT NULL, principal_id TEXT NOT NULL, perm TEXT NOT NULL, PRIMARY KEY(album_id, principal_kind, principal_id));
             CREATE TABLE IF NOT EXISTS meta_shares(id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta_sources(id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn rows<T: for<'de> Deserialize<'de>>(&self, sql: &str) -> Result<Vec<T>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let data: String = row.get(0)?;
                Ok(data)
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            let data: String = r.map_err(|e| e.to_string())?;
            out.push(serde_json::from_str(&data).map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    fn upsert_json(&self, table: &str, key_col: &str, key: &str, value: &impl Serialize) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!(
                "INSERT INTO {table}({key_col}, data) VALUES(?1, ?2)
                 ON CONFLICT({key_col}) DO UPDATE SET data=?2"
            ),
            rusqlite::params![key, serde_json::to_string(value).map_err(|e| e.to_string())?],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl MetaStore for SqliteMeta {
    fn list_users(&self) -> Result<Vec<User>, String> {
        self.rows::<User>("SELECT data FROM meta_users ORDER BY uid")
    }
    fn save_user(&self, user: &User) -> Result<(), String> {
        self.upsert_json("meta_users", "uid", &user.uid, user)
    }
    fn delete_user(&self, uid: &str) -> Result<(), String> {
        self.conn.lock().unwrap().execute("DELETE FROM meta_users WHERE uid=?1", [uid]).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_groups(&self) -> Result<Vec<Group>, String> {
        self.rows::<Group>("SELECT data FROM meta_groups ORDER BY gid")
    }
    fn save_groups(&self, groups: &[Group]) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM meta_groups", []).map_err(|e| e.to_string())?;
        for g in groups {
            conn.execute(
                "INSERT INTO meta_groups(gid, data) VALUES(?1, ?2)",
                rusqlite::params![g.gid, serde_json::to_string(g).map_err(|e| e.to_string())?],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn list_acls(&self) -> Result<Vec<AclEntry>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT album_id, principal_kind, principal_id, perm FROM meta_acls")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AclRow {
                    album_id: row.get(0)?,
                    principal_kind: row.get(1)?,
                    principal_id: row.get(2)?,
                    perm: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            let row: AclRow = r.map_err(|e| e.to_string())?;
            out.push(AclEntry {
                album_id: row.album_id,
                principal: crate::acl::Principal { kind: row.principal_kind, id: row.principal_id },
                perm: crate::acl::Perm::parse(&row.perm).unwrap_or(crate::acl::Perm::None),
            });
        }
        Ok(out)
    }
    fn upsert_acl(&self, entry: &AclEntry) -> Result<(), String> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO meta_acls(album_id, principal_kind, principal_id, perm) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(album_id, principal_kind, principal_id) DO UPDATE SET perm=?4",
                rusqlite::params![entry.album_id, entry.principal.kind, entry.principal.id, format!("{:?}", entry.perm).to_lowercase()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn delete_acl(&self, album_id: &str, kind: &str, id: &str) -> Result<(), String> {
        self.conn
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM meta_acls WHERE album_id=?1 AND principal_kind=?2 AND principal_id=?3",
                rusqlite::params![album_id, kind, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_shares(&self) -> Result<Vec<Share>, String> {
        self.rows::<Share>("SELECT data FROM meta_shares ORDER BY id")
    }
    fn save_share(&self, share: &Share) -> Result<(), String> {
        self.upsert_json("meta_shares", "id", &share.id, share)
    }
    fn delete_share(&self, id: &str) -> Result<(), String> {
        self.conn.lock().unwrap().execute("DELETE FROM meta_shares WHERE id=?1", [id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_sources(&self) -> Result<Vec<serde_json::Value>, String> {
        self.rows::<serde_json::Value>("SELECT data FROM meta_sources ORDER BY id")
    }
    fn save_sources(&self, sources: &[serde_json::Value]) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM meta_sources", []).map_err(|e| e.to_string())?;
        for s in sources {
            conn.execute(
                "INSERT INTO meta_sources(data) VALUES(?1)",
                rusqlite::params![serde_json::to_string(s).map_err(|e| e.to_string())?],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

// ---------------- MySQL（可选） ----------------

#[cfg(feature = "mysql")]
pub mod mysql_impl {
    use super::{MetaStore, AclEntry};
    use crate::auth::{Group, User};
    use crate::share::Share;
    use mysql_async::prelude::*;
    use mysql_async::{Pool, Opts};

    pub struct MysqlMeta {
        pool: Pool,
    }

    async fn exec(pool: &Pool, sql: &str, params: Vec<mysql_async::Value>) -> Result<(), String> {
        let mut conn = pool.get_conn().await.map_err(|e| e.to_string())?;
        conn.exec_drop(sql, params).await.map_err(|e| e.to_string())
    }

    async fn query_json(pool: &Pool, sql: &str) -> Result<Vec<String>, String> {
        let mut conn = pool.get_conn().await.map_err(|e| e.to_string())?;
        let rows: Vec<(String,)> = conn.query(sql).await.map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|(d,)| d).collect())
    }

    fn parse_list<T: for<'de> Deserialize<'de>>(rows: Vec<String>) -> Result<Vec<T>, String> {
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .collect()
    }

    impl MysqlMeta {
        pub async fn open(url: &str) -> Result<Self, String> {
            let opts = Opts::from_url(url).map_err(|e| e.to_string())?;
            let pool = Pool::new(opts);
            // 建表（幂等）
            for ddl in [
                "CREATE TABLE IF NOT EXISTS meta_users(uid VARCHAR(64) PRIMARY KEY, data JSON NOT NULL)",
                "CREATE TABLE IF NOT EXISTS meta_groups(gid VARCHAR(64) PRIMARY KEY, data JSON NOT NULL)",
                "CREATE TABLE IF NOT EXISTS meta_acls(album_id VARCHAR(64) NOT NULL, principal_kind VARCHAR(16) NOT NULL, principal_id VARCHAR(64) NOT NULL, perm VARCHAR(16) NOT NULL, PRIMARY KEY(album_id, principal_kind, principal_id))",
                "CREATE TABLE IF NOT EXISTS meta_shares(id VARCHAR(64) PRIMARY KEY, data JSON NOT NULL)",
                "CREATE TABLE IF NOT EXISTS meta_sources(id INT AUTO_INCREMENT PRIMARY KEY, data JSON NOT NULL)",
            ] {
                exec(&pool, ddl, vec![]).await?;
            }
            Ok(Self { pool })
        }
    }

    impl MetaStore for MysqlMeta {
        fn list_users(&self) -> Result<Vec<User>, String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                let rows = query_json(&pool, "SELECT data FROM meta_users ORDER BY uid").await?;
                parse_list::<User>(rows)
            })
        }
        fn save_user(&self, user: &User) -> Result<(), String> {
            let pool = self.pool.clone();
            let data = serde_json::to_string(user).map_err(|e| e.to_string())?;
            futures_block(move || async move {
                exec(&pool, "INSERT INTO meta_users(uid,data) VALUES(?,?) ON DUPLICATE KEY UPDATE data=VALUES(data)", vec![user.uid.clone().into(), data.into()]).await
            })
        }
        fn delete_user(&self, uid: &str) -> Result<(), String> {
            let pool = self.pool.clone();
            futures_block(move || async move { exec(&pool, "DELETE FROM meta_users WHERE uid=?", vec![uid.into()]).await })
        }
        fn list_groups(&self) -> Result<Vec<Group>, String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                let rows = query_json(&pool, "SELECT data FROM meta_groups ORDER BY gid").await?;
                parse_list::<Group>(rows)
            })
        }
        fn save_groups(&self, groups: &[Group]) -> Result<(), String> {
            let pool = self.pool.clone();
            let data = serde_json::to_string(groups).map_err(|e| e.to_string())?;
            futures_block(move || async move {
                exec(&pool, "DELETE FROM meta_groups", vec![]).await?;
                for g in groups {
                    exec(&pool, "INSERT INTO meta_groups(gid,data) VALUES(?,?)", vec![g.gid.clone().into(), data.clone().into()]).await?;
                }
                Ok(())
            })
        }
        fn list_acls(&self) -> Result<Vec<AclEntry>, String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                let mut conn = pool.get_conn().await.map_err(|e| e.to_string())?;
                let rows: Vec<(String, String, String, String)> = conn
                    .query("SELECT album_id, principal_kind, principal_id, perm FROM meta_acls")
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(rows
                    .into_iter()
                    .map(|(album_id, kind, id, perm)| AclEntry {
                        album_id,
                        principal: crate::acl::Principal { kind, id },
                        perm: crate::acl::Perm::parse(&perm).unwrap_or(crate::acl::Perm::None),
                    })
                    .collect())
            })
        }
        fn upsert_acl(&self, entry: &AclEntry) -> Result<(), String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                exec(&pool,
                    "INSERT INTO meta_acls(album_id,principal_kind,principal_id,perm) VALUES(?,?,?,?) ON DUPLICATE KEY UPDATE perm=VALUES(perm)",
                    vec![entry.album_id.clone().into(), entry.principal.kind.clone().into(), entry.principal.id.clone().into(), format!("{:?}", entry.perm).to_lowercase().into()],
                ).await
            })
        }
        fn delete_acl(&self, album_id: &str, kind: &str, id: &str) -> Result<(), String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                exec(&pool, "DELETE FROM meta_acls WHERE album_id=? AND principal_kind=? AND principal_id=?", vec![album_id.into(), kind.into(), id.into()]).await
            })
        }
        fn list_shares(&self) -> Result<Vec<Share>, String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                let rows = query_json(&pool, "SELECT data FROM meta_shares ORDER BY id").await?;
                parse_list::<Share>(rows)
            })
        }
        fn save_share(&self, share: &Share) -> Result<(), String> {
            let pool = self.pool.clone();
            let data = serde_json::to_string(share).map_err(|e| e.to_string())?;
            futures_block(move || async move {
                exec(&pool, "INSERT INTO meta_shares(id,data) VALUES(?,?) ON DUPLICATE KEY UPDATE data=VALUES(data)", vec![share.id.clone().into(), data.into()]).await
            })
        }
        fn delete_share(&self, id: &str) -> Result<(), String> {
            let pool = self.pool.clone();
            futures_block(move || async move { exec(&pool, "DELETE FROM meta_shares WHERE id=?", vec![id.into()]).await })
        }
        fn list_sources(&self) -> Result<Vec<serde_json::Value>, String> {
            let pool = self.pool.clone();
            futures_block(move || async move {
                let rows = query_json(&pool, "SELECT data FROM meta_sources ORDER BY id").await?;
                parse_list::<serde_json::Value>(rows)
            })
        }
        fn save_sources(&self, sources: &[serde_json::Value]) -> Result<(), String> {
            let pool = self.pool.clone();
            let data = serde_json::to_string(sources).map_err(|e| e.to_string())?;
            futures_block(move || async move {
                exec(&pool, "DELETE FROM meta_sources", vec![]).await?;
                for s in sources {
                    exec(&pool, "INSERT INTO meta_sources(data) VALUES(?)", vec![serde_json::to_string(s).map_err(|e| e.to_string())?.into()]).await?;
                }
                let _ = data;
                Ok(())
            })
        }
    }

    // 同步 trait 桥接：独立后台线程 + channel（MetaStore 是同步 trait）
    fn futures_block<F, T>(f: F) -> Result<T, String>
    where
        F: FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>>>> + Send + 'static,
        T: Send + 'static,
    {
        std::thread::scope(|s| -> Result<T, String> {
            let h = s.spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
                rt.block_on(f())
            });
            h.join().map_err(|_| "mysql worker panic".to_string())?
        })
    }
}

// ---------------- JSON 迁移（首次启动：users.json 等一次性导入后重命名保留） ----------------

pub fn migrate_json_files_if_present(meta: &dyn MetaStore, root: &std::path::Path) {
    let load = |name: &str| -> Option<String> { std::fs::read(root.join(name)).ok().and_then(|b| String::from_utf8(b).ok()) };
    // users
    if let Some(raw) = load("users.json") {
        if let Ok(users) = serde_json::from_str::<Vec<User>>(&raw) {
            let existing = meta.list_users().unwrap_or_default().len();
            if existing == 0 {
                for u in &users {
                    let _ = meta.save_user(u);
                }
                eprintln!("[meta] users.json -> 数据库（{} 个用户）", users.len());
            }
            let _ = std::fs::rename(root.join("users.json"), root.join("users.json.migrated"));
        }
    }
    if let Some(raw) = load("groups.json") {
        if let Ok(groups) = serde_json::from_str::<Vec<Group>>(&raw) {
            let existing = meta.list_groups().unwrap_or_default().len();
            if existing == 0 {
                let _ = meta.save_groups(&groups);
                eprintln!("[meta] groups.json -> 数据库（{} 个组）", groups.len());
            }
            let _ = std::fs::rename(root.join("groups.json"), root.join("groups.json.migrated"));
        }
    }
    if let Some(raw) = load("acls.json") {
        if let Ok(entries) = serde_json::from_str::<Vec<AclEntry>>(&raw) {
            let existing = meta.list_acls().unwrap_or_default().len();
            if existing == 0 {
                for e in &entries {
                    let _ = meta.upsert_acl(e);
                }
                eprintln!("[meta] acls.json -> 数据库（{} 条）", entries.len());
            }
            let _ = std::fs::rename(root.join("acls.json"), root.join("acls.json.migrated"));
        }
    }
    if let Some(raw) = load("shares.json") {
        if let Ok(shares) = serde_json::from_str::<Vec<Share>>(&raw) {
            let existing = meta.list_shares().unwrap_or_default().len();
            if existing == 0 {
                for s in &shares {
                    let _ = meta.save_share(s);
                }
                eprintln!("[meta] shares.json -> 数据库（{} 条）", shares.len());
            }
            let _ = std::fs::rename(root.join("shares.json"), root.join("shares.json.migrated"));
        }
    }
    if let Some(raw) = load("sources.json") {
        if let Ok(sources) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) {
            let existing = meta.list_sources().unwrap_or_default().len();
            if existing == 0 {
                let _ = meta.save_sources(&sources);
                eprintln!("[meta] sources.json -> 数据库（{} 个源）", sources.len());
            }
            let _ = std::fs::rename(root.join("sources.json"), root.join("sources.json.migrated"));
        }
    }
}

/// 打开元数据库：按配置选择 sqlite/mysql；并做 JSON 文件一次性迁移。
pub fn open_meta(cfg: &crate::config::ServerConfig, root: &std::path::Path) -> Result<Arc<dyn MetaStore>, String> {
    let store: Arc<dyn MetaStore> = match (cfg.database.driver.as_deref(), &cfg.database.url) {
        (Some("mysql"), Some(url)) => {
            #[cfg(feature = "mysql")]
            {
                let m = futures_block_once(async move { MysqlMeta::open(url).await })?;
                Arc::new(m)
            }
            #[cfg(not(feature = "mysql"))]
            return Err("编译时未启用 mysql feature，无法使用 mysql 元数据驱动".into());
        }
        _ => Arc::new(SqliteMeta::open(&root.join("meta.db"))?),
    };
    migrate_json_files_if_present(store.as_ref(), root);
    Ok(store)
}

fn futures_block_once<F>(fut: F) -> Result<F::Output, String>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let h = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(rt.block_on(fut))
    });
    h.join().map_err(|_| "worker panic".to_string())?
}

#[allow(unused)]
fn _hash_use() -> sha2::Sha256 { sha2::Sha256::new() }
