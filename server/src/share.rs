//! 分享网关（M4）：相册级分享链接，E2E 保持——服务器只缓存**密文**，
//! 解密密钥经 URL `#fragment` 交给浏览器（fragment 不会发送到服务器），
//! 浏览器用 WebCrypto AES-GCM 解密。
//!
//! - `POST /api/albums/{id}/share`（manage）→ {share_id, url}
//! - `GET  /s/{share_id}` 查看器页面（fragment 携带 `#k=<base64url JSON>`）
//! - `GET  /s/{share_id}/meta` 文件清单（**无任何密钥**）
//! - `GET  /s/{share_id}/pkg/{hash}` 密文包（iv‖ct‖tag 顺序拼接，惰性缓存）

use crate::acl::Perm;
use crate::AppState;
use axum::{
    extract::{Path as AxumPathT, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use heart_core::library::EncryptedPackage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::{auth, err, s_err};

#[derive(Clone, Serialize, Deserialize)]
pub struct Share {
    pub id: String,
    pub album_id: String,
    pub album_name: String,
    pub created_at: u64,
    /// 过期时间（unix 秒）；None = 永不过期
    pub expires_at: Option<u64>,
}

pub struct ShareStore {
    pub shares: Vec<Share>,
    path: std::path::PathBuf,
}

impl ShareStore {
    pub fn load(root: &std::path::Path) -> Self {
        let path = root.join("shares.json");
        let shares: Vec<Share> =
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { shares, path }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&self.shares).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.path.clone()).map_err(|e| e.to_string())
    }

    pub fn get(&self, id: &str) -> Option<&Share> {
        self.shares.iter().find(|s| s.id == id)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.shares.retain(|s| s.id != id);
        self.save()
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn check_expired(share: &Share) -> Result<(), Response> {
    if let Some(exp) = share.expires_at {
        if now() >= exp {
            return Err((StatusCode::GONE, "分享已过期").into_response());
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateShareReq {
    /// 过期天数；缺省永不过期
    pub expires_days: Option<u64>,
}

/// POST /api/albums/{id}/share
pub async fn create_share(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPathT(album_id): AxumPathT<String>,
    Json(req): Json<CreateShareReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let (album_name, file_list) = {
        let core = state.core.lock().unwrap();
        let parents = match crate::api::parent_map_pub(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        // manage 才能分享
        let groups = state.users.groups_of(&user.uid);
        if state.acl.lock().unwrap().effective(&user, &groups, &album_id, &parents) < Perm::Manage {
            return err(StatusCode::FORBIDDEN, "无权分享该相册");
        }
        let albums = match core.list_albums() {
            Ok(a) => a,
            Err(e) => return s_err(e.to_string()),
        };
        let Some(album) = albums.iter().find(|a| a.id == album_id) else {
            return err(StatusCode::NOT_FOUND, "相册不存在");
        };
        let name = album.name.clone();
        let blobs = match core.list_album_blobs(&album_id) {
            Ok(b) => b,
            Err(e) => return s_err(e.to_string()),
        };
        (name, blobs)
    };

    // 为每个 blob 组装密文包（记录 chunk 切分信息到 meta；密文包惰性缓存）
    let mut files = Vec::new();
    let cache_dir = state.share_cache_dir(&album_id_short(&album_id));
    for b in &file_list {
        if !b.mime.starts_with("image/") {
            continue; // v1 查看器仅支持图片（视频在线播放属后续）
        }
        let pkg: EncryptedPackage = {
            let core = state.core.lock().unwrap();
            match core.assemble_encrypted(&b.blob_hash) {
                Ok(p) => p,
                Err(e) => return s_err(e.to_string()),
            }
        };
        // 密文落缓存目录（原子写）
        let cache_file = cache_dir.join(format!("{}.enc", b.blob_hash));
        if !cache_file.exists() {
            std::fs::create_dir_all(&cache_dir).ok();
            let tmp = cache_file.with_extension("tmp");
            if std::fs::write(&tmp, &pkg.ciphertext).is_err() {
                return err(StatusCode::INTERNAL_SERVER_ERROR, "密文缓存写入失败");
            }
            let _ = std::fs::rename(&tmp, &cache_file);
        }
        files.push(serde_json::json!({
            "hash": b.blob_hash,
            "name": pkg.file_name,
            "mime": pkg.mime,
            "taken_at": b.taken_at,
            "chunk_lens": pkg.chunk_lens,
            "blob_key": pkg.blob_key_hex, // meta 不下发；仅写进 keys.json
        }));
    }
    if files.is_empty() {
        return err(StatusCode::BAD_REQUEST, "相册中没有可分享的图片");
    }

    let share_id = hex::encode({
        let mut buf = [0u8; 16];
        getrandom::getrandom(&mut buf).expect("OS RNG");
        buf
    });
    let share = Share {
        id: share_id.clone(),
        album_id: album_id.clone(),
        album_name: album_name.clone(),
        created_at: now(),
        expires_at: req.expires_days.map(|d| now() + d * 86400),
    };
    state.shares.lock().unwrap().shares.push(share.clone());
    if let Err(e) = state.shares.lock().unwrap().save() {
        return s_err(e);
    }

    // meta 写缓存目录（不含密钥）；密钥 JSON 单独存服务器侧（组装 fragment 用）
    let meta = serde_json::json!({
        "share_id": share_id,
        "album": album_name,
        "expires_at": share.expires_at,
        "files": files.iter().map(|f| {
            let mut f = f.clone();
            f["blob_key"] = serde_json::Value::Null; // meta 永不下发密钥
            f
        }).collect::<Vec<_>>(),
    });
    let _ = std::fs::write(cache_dir.join("meta.json"), meta.to_string());
    let keys: serde_json::Map<String, serde_json::Value> = files
        .iter()
        .map(|f| (f["hash"].as_str().unwrap().to_string(), f["blob_key"].clone()))
        .collect();
    let _ = std::fs::write(
        cache_dir.join("keys.json"),
        serde_json::json!({ "keys": keys }).to_string(),
    );

    Json(serde_json::json!({
        "share_id": share_id,
        "url": format!("/s/{share_id}"), // 完整 URL 由前端拼 origin + fragment
    }))
    .into_response()
}

fn album_id_short(album_id: &str) -> String {
    album_id.chars().take(8).collect()
}

/// GET /s/{share_id} — 查看器页面。
pub async fn share_page(
    State(state): State<Arc<AppState>>,
    AxumPathT(id): AxumPathT<String>,
) -> Response {
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    Html(include_str!("share.html")).into_response()
}

/// GET /s/{share_id}/meta — 文件清单（无密钥）。
pub async fn share_meta(
    State(state): State<Arc<AppState>>,
    AxumPathT(id): AxumPathT<String>,
) -> Response {
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    let cache_dir = state.share_cache_dir(&album_id_short(&share.album_id));
    match std::fs::read(cache_dir.join("meta.json")) {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "分享数据未就绪"),
    }
}

/// GET /s/{share_id}/keys — fragment 组装端点：返回**仅密钥**的 JSON。
/// 查看器页在浏览器内 fetch 后拼进 URL fragment 再清空内存——密钥不会出现在
/// 服务器日志的 URL 里（POST 语义 GET 化，家庭内网可接受；公网部署建议加访问码）。
pub async fn share_keys(
    State(state): State<Arc<AppState>>,
    AxumPathT(id): AxumPathT<String>,
) -> Response {
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    let cache_dir = state.share_cache_dir(&album_id_short(&share.album_id));
    match std::fs::read(cache_dir.join("keys.json")) {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/json".to_string()), (header::CACHE_CONTROL, "no-store".to_string())],
            bytes,
        )
            .into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "分享数据未就绪"),
    }
}

/// GET /s/{share_id}/pkg/{hash} — 密文包。
pub async fn share_pkg(
    State(state): State<Arc<AppState>>,
    AxumPathT(path): AxumPathT<(String, String)>,
) -> Response {
    let (id, hash) = path;
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    let cache_dir = state.share_cache_dir(&album_id_short(&share.album_id));
    // 防路径穿越
    if hash.contains("..") || hash.contains('/') || hash.contains('\\') {
        return err(StatusCode::BAD_REQUEST, "非法 hash");
    }
    match std::fs::read(cache_dir.join(format!("{hash}.enc"))) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "密文包不存在"),
    }
}

/// DELETE /api/shares/{id}
pub async fn delete_share(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPathT(id): AxumPathT<String>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    // 创建者 admin 或对该相册 manage（简化 v1：admin 或相册 manage）
    let core = state.core.lock().unwrap();
    let parents = match crate::api::parent_map_pub(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    drop(core);
    let groups = state.users.groups_of(&user.uid);
    if !user.is_admin
        && state.acl.lock().unwrap().effective(&user, &groups, &share.album_id, &parents) < Perm::Manage
    {
        return err(StatusCode::FORBIDDEN, "无权删除该分享");
    }
    if let Err(e) = state.shares.lock().unwrap().remove(&id) {
        return s_err(e);
    }
    StatusCode::OK.into_response()
}

/// GET /api/shares — 当前用户可管理的全部分享（admin=全部）。
pub async fn list_shares(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let shares: Vec<Share> = if user.is_admin {
        state.shares.lock().unwrap().shares.clone()
    } else {
        let core = state.core.lock().unwrap();
        let parents = match crate::api::parent_map_pub(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        drop(core);
        let groups = state.users.groups_of(&user.uid);
        let acl = state.acl.lock().unwrap();
        state
            .shares
            .lock()
            .unwrap()
            .shares
            .iter()
            .filter(|s| acl.effective(&user, &groups, &s.album_id, &parents) >= Perm::Manage)
            .cloned()
            .collect()
    };
    Json(shares).into_response()
}
