//! 分享网关（M4→M5a 增强）：
//! - 相册分享（POST /api/albums/{id}/share）与**单张/多选分享**（POST /api/shares）
//! - 链接分享**默认去元数据**：图片解密后重编码（剥离 EXIF/GPS）→ 新随机密钥
//!   按生产 chunk 格式重加密 → 服务器缓存的是「干净密文」；浏览器 WebCrypto 解密
//!   （AAD = blob_hash‖chunk_index‖total_chunks，tagLength 96）。
//! - 视频/不可解码图 v1：保留原密文（元数据未剥离，meta 标注 `sanitized:false`）。

use crate::acl::Perm;
use crate::AppState;
use axum::{
    extract::{Path as AxumPathT, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::Arc;

use crate::api::{auth, err, s_err};

/// blob → 其归属的相册 id 列表（家庭规模 O(相册×blob) 足够）。
fn blob_album_ids(core: &heart_core::library::HeartCore, hash: &str) -> Vec<String> {
    core.list_albums()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            core.list_album_blobs(&a.id)
                .map(|bs| bs.iter().any(|b| b.blob_hash == hash))
                .unwrap_or(false)
        })
        .map(|a| a.id)
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Share {
    pub id: String,
    /// album（按相册创建，blob 快照）| blobs（单张/多选）
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 相册分享的来源相册；blobs 分享为首个归属相册（缓存目录归属用）
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album_name: String,
    /// 分享内含的 blob 快照（创建时确定，后续相册变更不影响）
    #[serde(default)]
    pub blob_hashes: Vec<String>,
    #[serde(default)]
    pub created_at: u64,
    /// 过期时间（unix 秒）；None = 永不过期
    pub expires_at: Option<u64>,
    /// 分享码的 sha256（hex）；空 = 无分享码
    #[serde(default)]
    pub access_code_hash: String,
}

fn default_kind() -> String {
    "album".into()
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

/// 校验分享码（无分享码的分享直接放行；meta/keys/pkg 三个端点都要过）。
fn check_code(share: &Share, code: Option<&str>) -> Result<(), Response> {
    if share.access_code_hash.is_empty() {
        return Ok(());
    }
    let Some(code) = code else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "需要分享码", "need_code": true })),
        )
            .into_response());
    };
    let mut h = sha2::Sha256::new();
    sha2::Digest::update(&mut h, code.as_bytes());
    if hex::encode(sha2::Digest::finalize(h)) != share.access_code_hash {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "分享码错误", "need_code": true })),
        )
            .into_response());
    }
    Ok(())
}

fn random_id() -> String {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf).expect("OS RNG");
    hex::encode(buf)
}

/// 图片去元数据 + 新密钥重加密（生产 chunk 格式）。
/// 返回 (密文, 新 blob_key hex, chunk_lens, 有效 mime)。不可解码图返回 None（回退原密文）。
fn sanitize_and_encrypt(
    core: &heart_core::library::HeartCore,
    hash: &str,
    ) -> Result<Option<(Vec<u8>, String, Vec<u32>, String)>, String> {
    let (plain, manifest) = match core.decrypt_blob_bytes(hash) {
        Ok(v) => v,
        Err(e) => return Err(e.to_string()),
    };
    // 重编码剥离全部 EXIF/GPS；不可解码（如 HEIC）→ 回退
    let img = match image::load_from_memory(&plain) {
        Ok(i) => i,
        Err(_) => return Ok(None),
    };
    let out_mime = if manifest.mime == "image/jpeg" { "image/jpeg" } else { "image/png" };
    let mut buf: Vec<u8> = Vec::new();
    let enc_result = if out_mime == "image/jpeg" {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
        img.write_with_encoder(encoder)
    } else {
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
    };
    let _ = manifest;
    if let Err(e) = enc_result {
        return Err(format!("重编码失败: {e}"));
    }

    // 新随机密钥重加密（1 MiB 明文块；AAD 与生产一致）
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).expect("OS RNG");
    let hash_bytes: [u8; 32] = hex::decode(hash).map_err(|e| e.to_string())?.try_into().unwrap();
    let plain_chunks = buf.chunks(1_048_576);
    let total = plain_chunks.len() as u32;
    let mut ciphertext = Vec::with_capacity(buf.len() + total as usize * 24);
    let mut chunk_lens = Vec::with_capacity(plain_chunks.len());
    for (idx, chunk) in plain_chunks.enumerate() {
        let mut iv = [0u8; 12];
        getrandom::getrandom(&mut iv).expect("OS RNG");
        let ct = crypto::encrypt_chunk(&key, &iv, &hash_bytes, idx as u32, total, chunk)
            .map_err(|e| e.to_string())?;
        chunk_lens.push(ct.len() as u32);
        ciphertext.extend_from_slice(&ct);
    }
    Ok(Some((ciphertext, hex::encode(key), chunk_lens, out_mime.to_string())))
}

/// 组装分享内容（密文缓存 + meta/keys），返回 share 记录。
#[allow(clippy::too_many_arguments)]
fn build_share(
    state: &AppState,
    kind: &str,
    name: String,
    album_id: String,
    blob_hashes: Vec<String>,
    expires_days: Option<u64>,
    access_code: Option<&str>,
) -> Result<(Share, serde_json::Value), Response> {
    let share_id = random_id();
    let cache_dir = state.share_cache_dir(&share_id);
    std::fs::create_dir_all(&cache_dir).map_err(|e| s_err(e.to_string()))?;

    let mut files = Vec::new();
    let mut keys: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    {
        let core = state.core.lock().unwrap();
        let all = core.list_blobs().map_err(|e| s_err(e.to_string()))?;
        for hash in &blob_hashes {
            let Some(b) = all.iter().find(|b| &b.blob_hash == hash) else {
                return Err(err(StatusCode::NOT_FOUND, format!("照片 {hash} 不在索引中")));
            };
            let (ciphertext, key_hex, chunk_lens, mime, sanitized) =
                match sanitize_and_encrypt(&core, hash).map_err(s_err)? {
                    Some((ct, k, lens, mime)) => (ct, k, lens, mime, true),
                    None => {
                        // 不可解码图（如 HEIC）或视频：保留原密文，元数据未剥离
                        let pkg = core.assemble_encrypted(hash).map_err(|e| s_err(e.to_string()))?;
                        (pkg.ciphertext, pkg.blob_key_hex, pkg.chunk_lens, pkg.mime, false)
                    }
                };
            let cache_file = cache_dir.join(format!("{hash}.enc"));
            let tmp = cache_file.with_extension("tmp");
            std::fs::write(&tmp, &ciphertext).map_err(|e| s_err(e.to_string()))?;
            std::fs::rename(&tmp, &cache_file).map_err(|e| s_err(e.to_string()))?;
            keys.insert(hash.clone(), serde_json::Value::String(key_hex));
            files.push(serde_json::json!({
                "hash": hash,
                "name": b.file_name,
                "mime": mime,
                "taken_at": b.taken_at,
                "chunk_lens": chunk_lens,
                "sanitized": sanitized,
            }));
        }
    }
    if files.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "没有可分享的照片"));
    }

    let share = Share {
        id: share_id.clone(),
        kind: kind.to_string(),
        album_id,
        album_name: name.clone(),
        blob_hashes,
        created_at: now(),
        expires_at: expires_days.map(|d| now() + d * 86400),
        access_code_hash: match access_code.filter(|c| !c.is_empty()) {
            Some(code) => {
                let mut h = sha2::Sha256::new();
                sha2::Digest::update(&mut h, code.as_bytes());
                hex::encode(sha2::Digest::finalize(h))
            }
            None => String::new(),
        },
    };
    let meta = serde_json::json!({
        "share_id": share_id,
        "album": name,
        "expires_at": share.expires_at,
        "files": files,
    });
    let _ = std::fs::write(cache_dir.join("meta.json"), meta.to_string());
    let _ = std::fs::write(
        cache_dir.join("keys.json"),
        serde_json::json!({ "keys": keys }).to_string(),
    );
    Ok((share, meta))
}

/// 校验调用者对 hash 所属（任一）相册有 manage 权限。
fn require_manage_on_blobs(
    state: &AppState,
    user: &crate::auth::User,
    hashes: &[String],
) -> Result<(), Response> {
    let core = state.core.lock().unwrap();
    let parents = crate::api::parent_map_pub(&core)?;
    let groups = state.users.lock().unwrap().groups_of(&user.uid);
    let acl = state.acl.lock().unwrap();
    for h in hashes {
        let mut allowed = user.is_admin;
        if !allowed {
            for v in blob_album_ids(&core, h) {
                if acl.effective(user, &groups, &v, &parents) >= Perm::Manage {
                    allowed = true;
                    break;
                }
            }
        }
        if !allowed {
            return Err(err(StatusCode::FORBIDDEN, format!("无权分享照片 {h}")));
        }
    }
    Ok(())
}

/// POST /api/albums/{id}/share — 相册分享（快照：相册内全部图片）。
pub async fn create_album_share(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPathT(album_id): AxumPathT<String>,
    Json(req): Json<CreateShareReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let (album_name, hashes) = {
        let core = state.core.lock().unwrap();
        let parents = match crate::api::parent_map_pub(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let groups = state.users.lock().unwrap().groups_of(&user.uid);
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
        let hashes: Vec<String> = blobs
            .iter()
            .filter(|b| b.mime.starts_with("image/"))
            .map(|b| b.blob_hash.clone())
            .collect();
        (name, hashes)
    };
    if hashes.is_empty() {
        return err(StatusCode::BAD_REQUEST, "相册中没有可分享的图片");
    }
    let (share, _meta) = match build_share(
        &state,
        "album",
        album_name,
        album_id,
        hashes,
        req.expires_days,
        req.access_code.as_deref(),
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.shares.lock().unwrap().shares.push(share.clone());
    if let Err(e) = state.shares.lock().unwrap().save() {
        return s_err(e);
    }
    Json(serde_json::json!({ "share_id": share.id, "url": format!("/s/{}", share.id) })).into_response()
}

#[derive(Deserialize)]
pub struct CreateShareReq {
    /// 过期天数；缺省永不过期
    pub expires_days: Option<u64>,
    /// 可选分享码（访问者需输入）
    pub access_code: Option<String>,
}

/// POST /api/shares — 单张/多选分享（去元数据）。
#[derive(Deserialize)]
pub struct CreateBlobsShareReq {
    pub blob_hashes: Vec<String>,
    pub expires_days: Option<u64>,
    /// 分享标题（默认用首个文件名）
    pub name: Option<String>,
    /// 可选分享码
    pub access_code: Option<String>,
}

pub async fn create_blobs_share(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateBlobsShareReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if req.blob_hashes.is_empty() {
        return err(StatusCode::BAD_REQUEST, "未选择照片");
    }
    let hashes: Vec<String> = req.blob_hashes.iter().map(|h| h.to_lowercase()).collect();
    if let Err(e) = require_manage_on_blobs(&state, &user, &hashes) {
        return e;
    }
    let name = req.name.clone().unwrap_or_else(|| format!("{} 张照片", hashes.len()));
    let album_id = first_album_of(&state, &hashes).unwrap_or_default();
    let (share, _meta) = match build_share(
        &state,
        "blobs",
        name,
        album_id,
        hashes,
        req.expires_days,
        req.access_code.as_deref(),
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };
    state.shares.lock().unwrap().shares.push(share.clone());
    if let Err(e) = state.shares.lock().unwrap().save() {
        return s_err(e);
    }
    Json(serde_json::json!({ "share_id": share.id, "url": format!("/s/{}", share.id) })).into_response()
}

fn first_album_of(state: &AppState, hashes: &[String]) -> Option<String> {
    let core = state.core.lock().unwrap();
    for h in hashes {
        for a in blob_album_ids(&core, h) {
            return Some(a);
        }
    }
    None
}

/// GET /s/{share_id} — 查看器页面。
pub async fn share_page(State(state): State<Arc<AppState>>, AxumPathT(id): AxumPathT<String>) -> Response {
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
    Query(code): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    if let Err(e) = check_code(&share, code.get("code").map(|s| s.as_str())) {
        return e;
    }
    let cache_dir = state.share_cache_dir(&share.id);
    match std::fs::read(cache_dir.join("meta.json")) {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/json".to_string())],
            bytes,
        )
            .into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "分享数据未就绪"),
    }
}

/// GET /s/{share_id}/keys — 密钥 JSON（查看器取用；no-store）。
pub async fn share_keys(
    State(state): State<Arc<AppState>>,
    AxumPathT(id): AxumPathT<String>,
    Query(code): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    if let Err(e) = check_code(&share, code.get("code").map(|s| s.as_str())) {
        return e;
    }
    let cache_dir = state.share_cache_dir(&share.id);
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
    Query(code): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let (id, hash) = path;
    let Some(share) = state.shares.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "分享不存在");
    };
    if let Err(e) = check_expired(&share) {
        return e;
    }
    if let Err(e) = check_code(&share, code.get("code").map(|s| s.as_str())) {
        return e;
    }
    if hash.contains("..") || hash.contains('/') || hash.contains('\\') {
        return err(StatusCode::BAD_REQUEST, "非法 hash");
    }
    let cache_dir = state.share_cache_dir(&share.id);
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
    let core = state.core.lock().unwrap();
    let parents = match crate::api::parent_map_pub(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    drop(core);
    let groups = state.users.lock().unwrap().groups_of(&user.uid);
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
pub async fn list_shares(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Response {
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
        let groups = state.users.lock().unwrap().groups_of(&user.uid);
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
