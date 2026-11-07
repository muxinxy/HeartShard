//! 导出镜像：相册树 → 普通目录。
//! 按相册层级落盘，每个 blob 走 export_blob（完整 RS 重组 + SHA-256 校验）。

use crate::Res;
use heart_core::library::HeartCore;
use std::collections::HashMap;
use std::path::PathBuf;

use super::unlock_library;
use crate::Config;

/// 在相册树中按名称查找（精确匹配；返回自身+全部子孙的 (相册路径, id)）。
fn collect_subtree(
    core: &mut HeartCore,
    root_name: &str,
) -> Res<Vec<(String, String)>> {
    let albums = core.list_albums().map_err(|e| e.to_string())?;
    let by_parent: HashMap<Option<String>, Vec<_>> = {
        let mut m: HashMap<Option<String>, Vec<_>> = HashMap::new();
        for a in &albums {
            m.entry(a.parent_id.clone()).or_default().push(a);
        }
        m
    };
    let roots: Vec<_> = albums.iter().filter(|a| a.name == root_name).collect();
    let Some(first) = roots.first() else {
        return Err(format!("相册「{root_name}」不存在"));
    };
    let mut out = Vec::new();
    fn visit(
        id: &str,
        path: &str,
        by_parent: &HashMap<Option<String>, Vec<&heart_core::library::AlbumInfo>>,
        out: &mut Vec<(String, String)>,
    ) {
        out.push((path.to_string(), id.to_string()));
        for child in by_parent.get(&Some(id.to_string())).into_iter().flatten() {
            visit(&child.id, &format!("{path}/{}", child.name), by_parent, out);
        }
    }
    visit(&first.id, root_name, &by_parent, &mut out);
    Ok(out)
}

pub fn run_export(cfg: &Config, album: &str, to: &PathBuf) -> Res<usize> {
    let mut core = unlock_library(cfg)?;
    let tree = collect_subtree(&mut core, album)?;

    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    for (rel, album_id) in &tree {
        let dir = to.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for b in core.list_album_blobs(album_id).map_err(|e| e.to_string())? {
            let dest = dir.join(&b.file_name);
            core.export_blob(&b.blob_hash, &dest).map_err(|e| {
                format!("导出 {} 失败: {e}", b.file_name)
            })?;
            count += 1;
        }
    }
    Ok(count)
}
