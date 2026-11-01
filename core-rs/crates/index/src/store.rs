//! BlobStore trait——云存储的被动 Blob 视图（put/get/list/delete）。
//!
//! M2 提供本地目录实现（模拟 Storage / M1 的落盘语义）；
//! S3/WebDAV/OpenList/FTP 适配器在平台层（src-tauri）实现同一 trait。

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("network/protocol: {0}")]
    Protocol(String),
    #[error("remote key rejected: {0}")]
    BadKey(String),
}

/// 只暴露四个最朴素的原语——足够承载 shard/manifest/远端索引。
/// 实现必须保证：`get` 返回 `None` 表示对象不存在（而非错误）。
pub trait BlobStore: Send + Sync {
    /// Storage 标识（诊断/报告用，如 "local-mock"、"my-s3-bucket"）。
    fn id(&self) -> &str;
    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError>;
    fn get(&self, remote_key: &str) -> Option<Vec<u8>>;
    /// 列出 `prefix` 下的所有对象键（不含 prefix 本身）。
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
    fn delete(&self, remote_key: &str) -> Result<(), StoreError>;
}

/// 校验对象键段（复用 keypath 语义，防御路径逃逸）。
fn validate_key(remote_key: &str) -> Result<(), StoreError> {
    for seg in remote_key.split('/') {
        keypath::sanitize_key_segment(seg)
            .map_err(|e| StoreError::BadKey(format!("{remote_key}: {e}")))?;
    }
    Ok(())
}

fn key_to_path(root: &Path, remote_key: &str) -> Result<PathBuf, StoreError> {
    validate_key(remote_key)?;
    let mut p = root.to_path_buf();
    for seg in remote_key.split('/') {
        p.push(seg);
    }
    Ok(p)
}

/// 本地目录 Storage（模拟云端；M2 双 Storage 即两个不同根目录）。
pub struct LocalDirStore {
    root: PathBuf,
    id: String,
}

impl LocalDirStore {
    pub fn new(root: impl Into<PathBuf>, id: impl Into<String>) -> Self {
        Self { root: root.into(), id: id.into() }
    }

    /// 遍历 prefix 对应目录下的全部文件，返回相对 key。
    fn walk_files(dir: &Path, rel: &mut String, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_dir() {
                let saved = rel.len();
                if !rel.is_empty() {
                    rel.push('/');
                }
                rel.push_str(&name);
                Self::walk_files(&entry.path(), rel, out);
                rel.truncate(saved);
            } else {
                let saved = rel.len();
                if !rel.is_empty() {
                    rel.push('/');
                }
                rel.push_str(&name);
                out.push(rel.clone());
                rel.truncate(saved);
            }
        }
    }
}

impl BlobStore for LocalDirStore {
    fn id(&self) -> &str {
        &self.id
    }

    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError> {
        let path = key_to_path(&self.root, remote_key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("part");
        fs::write(&tmp, data)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn get(&self, remote_key: &str) -> Option<Vec<u8>> {
        let path = key_to_path(&self.root, remote_key).ok()?;
        fs::read(path).ok()
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        validate_key(prefix)?;
        let mut out = Vec::new();
        if prefix.is_empty() {
            Self::walk_files(&self.root, &mut String::new(), &mut out);
        } else {
            let dir = key_to_path(&self.root, prefix)?;
            let mut rel = prefix.trim_end_matches('/').to_string();
            Self::walk_files(&dir, &mut rel, &mut out);
        }
        out.sort();
        Ok(out)
    }

    fn delete(&self, remote_key: &str) -> Result<(), StoreError> {
        let path = key_to_path(&self.root, remote_key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_list_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalDirStore::new(dir.path(), "mock-a");
        assert_eq!(store.id(), "mock-a");

        store.put("blobs/ab/cd/00000_00.shard", b"hello").unwrap();
        assert_eq!(store.get("blobs/ab/cd/00000_00.shard").as_deref(), Some(b"hello".as_slice()));
        assert_eq!(store.get("blobs/ab/cd/99999_99.shard"), None);

        store.put("index/dev-a/1.idx", b"x").unwrap();
        store.put("index/dev-a/2.idx", b"y").unwrap();
        store.put("index/dev-b/1.idx", b"z").unwrap();
        let keys = store.list("index").unwrap();
        assert_eq!(
            keys,
            vec![
                "index/dev-a/1.idx".to_string(),
                "index/dev-a/2.idx".to_string(),
                "index/dev-b/1.idx".to_string(),
            ]
        );

        store.delete("index/dev-a/1.idx").unwrap();
        assert_eq!(store.get("index/dev-a/1.idx"), None);
        // 删除不存在 = 幂等成功
        store.delete("index/dev-a/1.idx").unwrap();
    }

    #[test]
    fn rejects_path_escape_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalDirStore::new(dir.path(), "mock-a");
        assert!(matches!(store.put("blobs/../evil", b"x"), Err(StoreError::BadKey(_))));
        assert!(matches!(store.put("blobs/CON/x", b"x"), Err(StoreError::BadKey(_))));
        assert!(matches!(store.put("blobs/大写/x", b"x"), Err(StoreError::BadKey(_))));
        // 空段：get 侧防御性地返回 None，put/list 显式报错
        assert_eq!(store.get("blobs//x"), None);
        assert!(store.list("index//x").is_err());
    }
}
