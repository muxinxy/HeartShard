//! 同步引擎：扫描源 → 三层 diff → 摄取/移动/删除 → 清单落盘。
//!
//! 语义（docs/development.md 同步工具章节）：
//! - 三层 diff：path+size+mtime 快筛 → SHA-256 精确 → 相册索引去重
//! - `append`：只新增；源端修改/删除不影响相册（照片是永久记忆）
//! - `mirror`：镜像源——修改进新版本（旧版进回收站）、删除跟进（keep/trash）
//! - 移动/改名（同 hash 异路径）：**只改相册关联，不重传任何字节**
//! - 收编：源里清单外的新文件自动按目录树映射为相册结构

use crate::{SourceConfig, Res};
use heart_core::library::{HeartCore, ImportStatus};
use heart_core::storage_s3::S3Store;
use heart_core::storage_openlist::OpenListStore;
use heart_core::storage_webdav::WebDavStore;
use index::store::BlobStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 媒体扩展名白名单（与桌面版导入一致；超出范围由 mime 判定兜底拒绝）。
const MEDIA_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "heif", "avif", "tif", "tiff",
    "mp4", "mov", "m4v", "avi", "mkv", "webm",
];

#[derive(Debug, Default)]
pub struct Report {
    pub added: usize,
    pub moved: usize,
    pub removed: usize,
}

/// 清单条目：源端文件的最后已知状态。
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    size: u64,
    mtime: i64,
    sha256: String,
    blob_hash: Option<String>,
    album_path: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    entries: HashMap<String, Entry>,
}

/// 扫描到的源文件。
#[derive(Clone)]
struct ScanItem {
    /// 相对路径（'/' 分隔，清单主键）
    key: String,
    size: u64,
    mtime: i64,
}

// ---------------------------------------------------------------- 扫描

fn is_media(p: &str) -> bool {
    let ext = p.rsplit('.').next().unwrap_or("").to_lowercase();
    MEDIA_EXT.contains(&ext.as_str())
}

fn scan_local(root: &str) -> Res<Vec<ScanItem>> {
    let base = PathBuf::from(root);
    if !base.is_dir() {
        return Err(format!("本地源不存在或不是目录: {root}"));
    }
    let mut out = Vec::new();
    walk(&base, &base, &mut out)?;
    Ok(out)
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<ScanItem>) -> Res<()> {
    for e in std::fs::read_dir(dir).map_err(|e| format!("读目录 {}: {e}", dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.is_dir() {
            walk(base, &p, out)?;
        } else if p.is_file() {
            let rel = p
                .strip_prefix(base)
                .map_err(|_| "strip_prefix failed")?
                .to_string_lossy()
                .replace('\\', "/");
            if !is_media(&rel) {
                continue;
            }
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(ScanItem {
                key: rel,
                size: meta.len(),
                mtime,
            });
        }
    }
    Ok(())
}

/// 远端源：BlobStore::list 全量键 → 过滤媒体扩展。mtime 不可知（记 0）。
fn scan_remote(src: &SourceConfig) -> Res<Vec<ScanItem>> {
    let keys: Vec<String> = match src.kind.as_str() {
        "s3" => {
            let cfg = src.s3.as_ref().ok_or("s3 源缺少 [source.s3] 配置")?;
            S3Store::new(cfg.clone())
                .map_err(|e| format!("S3 初始化失败: {e}"))?
                .list("")
                .map_err(|e| format!("S3 列举失败: {e}"))?
        }
        "webdav" => {
            let cfg = src.webdav.as_ref().ok_or("webdav 源缺少 [source.webdav] 配置")?;
            WebDavStore::new(cfg.clone())
                .map_err(|e| format!("WebDAV 初始化失败: {e}"))?
                .list("")
                .map_err(|e| format!("WebDAV 列举失败: {e}"))?
        }
        "openlist" => {
            let cfg = src.openlist.as_ref().ok_or("openlist 源缺少 [source.openlist] 配置")?;
            OpenListStore::new(cfg.clone())
                .map_err(|e| format!("OpenList 初始化失败: {e}"))?
                .list("")
                .map_err(|e| format!("OpenList 列举失败: {e}"))?
        }
        other => return Err(format!("未知源类型: {other}")),
    };
    Ok(keys
        .into_iter()
        .filter(|k| is_media(k) && !k.ends_with('/'))
        .map(|k| ScanItem { key: k, size: 0, mtime: 0 })
        .collect())
}

// ---------------------------------------------------------------- 相册链

/// 相册路径（a/b/c）→ id 的缓存；缺失的自动创建。
struct AlbumIndex {
    by_path: HashMap<String, String>,
}

impl AlbumIndex {
    fn load(core: &HeartCore) -> Res<Self> {
        let mut by_path = HashMap::new();
        // parent_id → 子相册列表
        let albums = core.list_albums().map_err(|e| e.to_string())?;
        let children: HashMap<Option<String>, Vec<_>> = {
            let mut m: HashMap<Option<String>, Vec<_>> = HashMap::new();
            for a in &albums {
                m.entry(a.parent_id.clone()).or_default().push(a);
            }
            m
        };
        // 从根做多序 DFS，拼路径
        fn visit(
            parent: Option<String>,
            prefix: &str,
            children: &HashMap<Option<String>, Vec<&heart_core::library::AlbumInfo>>,
            by_path: &mut HashMap<String, String>,
        ) {
            for a in children.get(&parent).into_iter().flatten() {
                let path = if prefix.is_empty() {
                    a.name.clone()
                } else {
                    format!("{prefix}/{}", a.name)
                };
                by_path.insert(path.clone(), a.id.clone());
                visit(Some(a.id.clone()), &path, children, by_path);
            }
        }
        visit(None, "", &children, &mut by_path);
        Ok(Self { by_path })
    }

    /// 确保相册链存在，返回末级相册 id。
    fn ensure(&mut self, core: &mut HeartCore, segments: &[&str]) -> Res<String> {
        let mut path = String::new();
        let mut parent_id: Option<String> = None;
        for seg in segments {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(seg);
            if let Some(id) = self.by_path.get(&path) {
                parent_id = Some(id.clone());
                continue;
            }
            let id = core.album_create(seg, parent_id.clone()).map_err(|e| e.to_string())?;
            self.by_path.insert(path.clone(), id.clone());
            parent_id = Some(id);
        }
        parent_id.ok_or_else(|| "空相册路径".to_string())
    }
}

fn album_segments(src: &SourceConfig, key: &str) -> Vec<String> {
    // 去掉文件名，只留目录段；album_root 作为挂载前缀
    let dir = match key.rfind('/') {
        Some(i) => &key[..i],
        None => "",
    };
    let mut segs: Vec<String> = Vec::new();
    if let Some(root) = &src.album_root {
        if !root.is_empty() {
            segs.push(root.clone());
        }
    }
    if !dir.is_empty() {
        segs.extend(dir.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").map(String::from));
    }
    segs
}

// ---------------------------------------------------------------- 内容哈希

fn hash_local(p: &Path) -> Res<String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("打开 {}: {e}", p.display()))?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(hex::encode(h.finalize()))
}

/// 远端对象下载到临时文件并返回哈希；返回 (临时路径, sha256)。
fn fetch_remote(
    src: &SourceConfig,
    key: &str,
    tmp_dir: &Path,
) -> Res<(PathBuf, String)> {
    let store: Box<dyn BlobStore> = match src.kind.as_str() {
        "s3" => {
            let cfg = src.s3.as_ref().ok_or("s3 源缺少配置")?;
            Box::new(S3Store::new(cfg.clone()).map_err(|e| e.to_string())?)
        }
        "webdav" => {
            let cfg = src.webdav.as_ref().ok_or("webdav 源缺少配置")?;
            Box::new(WebDavStore::new(cfg.clone()).map_err(|e| e.to_string())?)
        }
        "openlist" => {
            let cfg = src.openlist.as_ref().ok_or("openlist 源缺少配置")?;
            Box::new(OpenListStore::new(cfg.clone()).map_err(|e| e.to_string())?)
        }
        other => return Err(format!("未知源类型: {other}")),
    };
    let data = store.get(key).ok_or_else(|| format!("远端对象不存在: {key}"))?;
    let mut h = Sha256::new();
    h.update(&data);
    let sha = hex::encode(h.finalize());
    let fname = key.rsplit('/').next().unwrap_or("file.bin").to_string();
    let dst = tmp_dir.join(format!("{}_{}", uuid_like(), fname));
    std::fs::write(&dst, &data).map_err(|e| e.to_string())?;
    Ok((dst, sha))
}

fn uuid_like() -> String {
    // 简单时间戳+计数临时名（无 uuid 依赖）
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}{}", t, N.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------- 主流程

pub fn sync_source(core: &mut HeartCore, src: &SourceConfig, full: bool) -> Res<Report> {
    let strategy_mirror = src.strategy == "mirror";

    let data_dir = core.data_dir();
    let syncer_dir = data_dir.join("syncer");
    std::fs::create_dir_all(&syncer_dir).map_err(|e| e.to_string())?;
    let manifest_path = syncer_dir.join(format!("{}.manifest.json", src.id));
    let mut manifest: Manifest = std::fs::read(&manifest_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    // 源扫描
    let scan = match src.kind.as_str() {
        "local" => scan_local(src.root.as_deref().ok_or("local 源缺少 root 配置")?)?,
        _ => scan_remote(src)?,
    };
    let scanned_keys: HashSet<&str> = scan.iter().map(|s| s.key.as_str()).collect();

    let mut albums = AlbumIndex::load(core)?;
    let tmp_dir = std::env::temp_dir().join("heartshard-syncer");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let mut report = Report::default();

    // 同 sha256 → 新路径（移动/改名检测用）
    // 先对需要哈希的条目算 hash，构建 scan_sha 映射
    let mut scan_sha: HashMap<String, String> = HashMap::new(); // key → sha256

    // 第一轮：分类
    enum Action {
        New,
        Changed,
        Unchanged,
    }
    let mut actions: Vec<(String, Action, ScanItem)> = Vec::new();
    for item in &scan {
        match manifest.entries.get(&item.key) {
            None => actions.push((item.key.clone(), Action::New, item.clone())),
            Some(e) => {
                let suspicious = full || e.size != item.size || e.mtime != item.mtime;
                // 远端源 mtime 恒为 0：仅在 full 或 size 变化时重哈希
                if suspicious {
                    actions.push((item.key.clone(), Action::Changed, item.clone()));
                } else {
                    actions.push((item.key.clone(), Action::Unchanged, item.clone()));
                }
            }
        }
    }

    // 第二轮：处理新增/变更（需要哈希）
    for (key, action, item) in actions {
        let entry = manifest.entries.get(&key).cloned();
        match action {
            Action::Unchanged => {
                if let Some(e) = entry {
                    scan_sha.insert(key, e.sha256);
                }
                continue;
            }
            Action::New | Action::Changed => {}
        }

        // 取内容 + 哈希
        let (abs_path, sha, tmp_to_clean) = if src.kind == "local" {
            let root = src.root.as_deref().ok_or("local 源缺少 root")?;
            let p = Path::new(root).join(&key);
            let sha = hash_local(&p)?;
            (p, sha, None)
        } else {
            let (tmp, sha) = fetch_remote(src, &key, &tmp_dir)?;
            (tmp.clone(), sha, Some(tmp))
        };

        if let Action::Changed = action {
            if let Some(e) = &entry {
                if e.sha256 == sha {
                    // 内容没变（只是 mtime/size 抖动）：刷新清单
                    let mut e2 = e.clone();
                    e2.size = item.size;
                    e2.mtime = item.mtime;
                    manifest.entries.insert(key.clone(), e2);
                    scan_sha.insert(key, sha);
                    if let Some(t) = tmp_to_clean {
                        let _ = std::fs::remove_file(t);
                    }
                    continue;
                }
                // 内容真的变了
                if !strategy_mirror {
                    // append：保留旧版本，只刷新快筛字段，避免每轮重哈希
                    let mut e2 = e.clone();
                    e2.size = item.size;
                    e2.mtime = item.mtime;
                    scan_sha.insert(key.clone(), e2.sha256.clone());
                    manifest.entries.insert(key, e2);
                    if let Some(t) = tmp_to_clean {
                        let _ = std::fs::remove_file(t);
                    }
                    continue;
                }
                // mirror：旧 blob 移出相册并进回收站（见下，导入新版本后执行）
            }
        }

        // 导入（或跨源去重）
        let outcome = core.import_file(&abs_path).map_err(|e| e.to_string())?;
        if let Some(t) = tmp_to_clean {
            let _ = std::fs::remove_file(t);
        }
        let blob_hash = match outcome.status {
            ImportStatus::Imported | ImportStatus::SkippedDuplicate => {
                outcome.blob_hash.clone().ok_or("导入成功但缺少 blob_hash")?
            }
            ImportStatus::Failed => {
                eprintln!("[{}] 导入失败: {}", src.id, key);
                if let Some(e) = entry {
                    scan_sha.insert(key.clone(), e.sha256);
                }
                continue;
            }
        };

        // 相册映射
        let segs = album_segments(src, &key);
        let album_id = if segs.is_empty() {
            None
        } else {
            Some(albums.ensure(core, &segs.iter().map(|s| s.as_str()).collect::<Vec<_>>())?)
        };
        if let (Some(id), true) = (&album_id, segs.len() > 0) {
            core.album_add_blob(id, &blob_hash).map_err(|e| e.to_string())?;
        }

        // mirror + 内容变更：旧 blob 退役（移出相册 + 回收站）
        if let Some(old) = &entry {
            if old.blob_hash != Some(blob_hash.clone()) {
                if let Some(old_hash) = &old.blob_hash {
                    if old_hash != &blob_hash {
                        if let Some(id) = albums.by_path.get(&old.album_path).cloned() {
                            let _ = core.album_remove_blob(&id, old_hash);
                        }
                        let _ = core.delete_blob(old_hash); // 回收站（可恢复）
                    }
                }
            }
        }

        scan_sha.insert(key.clone(), sha.clone());
        manifest.entries.insert(
            key.clone(),
            Entry {
                size: item.size,
                mtime: item.mtime,
                sha256: sha,
                blob_hash: Some(blob_hash),
                album_path: segs.join("/"),
            },
        );
        report.added += 1;
    }

    // 第三轮：源端消失的条目（mirror：移动优先，其次删除；append：忽略）
    let missing: Vec<(String, Entry)> = manifest
        .entries
        .iter()
        .filter(|(k, _)| !scanned_keys.contains(k.as_str()))
        .map(|(k, e)| (k.clone(), e.clone()))
        .collect();
    for (key, e) in missing {
        if !strategy_mirror {
            continue; // append：源删除不影响相册
        }
        // 同 hash 是否出现在新位置（移动/改名）
        let moved_to = scan_sha
            .iter()
            .find(|(_, sha)| *sha == &e.sha256)
            .map(|(k, _)| k.clone());
        if let Some(new_key) = moved_to {
            // 只改相册关联
            if !e.album_path.is_empty() {
                if let Some(old_id) = albums.by_path.get(&e.album_path).cloned() {
                    if let Some(old_hash) = &e.blob_hash {
                        let _ = core.album_remove_blob(&old_id, old_hash);
                    }
                }
            }
            let segs = album_segments(src, &new_key);
            if !segs.is_empty() {
                let id = albums.ensure(
                    core,
                    &segs.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                )?;
                if let Some(old_hash) = &e.blob_hash {
                    let _ = core.album_add_blob(&id, old_hash);
                }
            }
            if let Some(mut e2) = manifest.entries.remove(&key) {
                e2.album_path = segs.join("/");
                manifest.entries.insert(new_key, e2);
            }
            report.moved += 1;
        } else {
            // 真删除
            match src.delete_mode.as_str() {
                "trash" => {
                    if let Some(h) = &e.blob_hash {
                        let _ = core.delete_blob(h);
                    }
                }
                _ => {} // keep：相册保留
            }
            manifest.entries.remove(&key);
            report.removed += 1;
        }
    }

    // 清单落盘（原子写）
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let tmp = manifest_path.with_extension("tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &manifest_path).map_err(|e| e.to_string())?;

    Ok(report)
}
