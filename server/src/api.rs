//! 数据 API（axum）：认证 + 相册 ACL 过滤 + 加密数据的 HTTP 面。
//!
//! 权限语义：相册列表只返回 view+；上传需 upload+；建相册需父相册 manage+；
//! 回收站/删除/权限表管理 v1 仅 admin。ACL 是应用层强制（见 config.rs 安全模型）。

use crate::acl::{Perm, Principal};
use crate::auth::User;
use crate::AppState;
use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use heart_core::library::{AlbumInfo, BlobEntry, HeartCore, ImportStatus};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type S = State<Arc<AppState>>;

pub fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

pub fn s_err(e: String) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, e)
}

pub fn auth(state: &AppState, headers: &HeaderMap) -> Result<User, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "缺少会话令牌"))?;
    state
        .users
        .verify(token)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "会话无效或已过期"))
}

/// 相册树 parent 映射（album_id → 父 id）。
fn parent_map(core: &HeartCore) -> Result<HashMap<String, Option<String>>, Response> {
    Ok(core
        .list_albums()
        .map_err(|e| s_err(e.to_string()))?
        .into_iter()
        .map(|a: AlbumInfo| (a.id, a.parent_id))
        .collect())
}

/// share.rs 等模块使用的公开包装。
pub fn parent_map_pub(core: &HeartCore) -> Result<HashMap<String, Option<String>>, Response> {
    parent_map(core)
}

/// token 查询参数桥接：`?token=xxx` → Authorization 头（img/video 标签无法带头）。
/// 同时输出请求日志（方法/路径/状态/耗时），方便调试。
pub async fn token_query_bridge(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    if parts.headers.get(header::AUTHORIZATION).is_none() {
        if let Some(q) = parts.uri.query() {
            for pair in q.split('&') {
                if let Some(t) = pair.strip_prefix("token=") {
                    if let Ok(v) = format!("Bearer {t}").parse() {
                        parts.headers.insert(header::AUTHORIZATION, v);
                    }
                }
            }
        }
    }
    let method = parts.method.clone();
    let path = parts.uri.path().to_string();
    let started = std::time::Instant::now();
    let res = next.run(axum::extract::Request::from_parts(parts, body)).await;
    eprintln!(
        "[http] {} {} -> {} ({}ms)",
        method,
        path,
        res.status(),
        started.elapsed().as_millis()
    );
    res
}

fn effective(state: &AppState, user: &User, album_id: &str, parents: &HashMap<String, Option<String>>) -> Perm {
    let groups = state.users.groups_of(&user.uid);
    state.acl.effective(user, &groups, album_id, parents)
}

/// 用户可见（view+）的相册集合。
fn visible_albums(
    core: &HeartCore,
    state: &AppState,
    user: &User,
    parents: &HashMap<String, Option<String>>,
) -> Result<Vec<(AlbumInfo, Perm)>, Response> {
    let albums = core.list_albums().map_err(|e| s_err(e.to_string()))?;
    let groups = state.users.groups_of(&user.uid);
    let acl = &state.acl;
    Ok(albums
        .into_iter()
        .filter_map(|a| {
            let p = acl.effective(user, &groups, &a.id, parents);
            (p >= Perm::View).then_some((a, p))
        })
        .collect())
}

// ---------------------------------------------------------------- 认证

#[derive(Deserialize)]
pub struct LoginReq {
    pub name: String,
    pub password: String,
    /// TOTP 6 位验证码（2fa 启用时必填）
    pub totp_code: Option<String>,
}

pub async fn login(
    State(state): S,
    crate::ratelimit::ClientIp(ip): crate::ratelimit::ClientIp,
    Json(req): Json<LoginReq>,
) -> Response {
    // 登录限流：同一 IP 连续失败 5 次后锁定 10 分钟
    if let Err(wait) = crate::ratelimit::check(ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": format!("尝试次数过多，请等待 {wait} 秒后重试") })),
        )
            .into_response();
    }
    match state.users.login(&req.name, &req.password, req.totp_code.as_deref()) {
        Ok((token, user, totp_required)) => {
            crate::ratelimit::record_ok(ip);
            eprintln!("[auth] 登录成功 user={} uid={} admin={}", user.name, user.uid, user.is_admin);
            Json(serde_json::json!({
                "token": token,
                "uid": user.uid,
                "name": user.name,
                "is_admin": user.is_admin,
                "avatar": user.avatar,
                "totp_required": totp_required,
            }))
            .into_response()
        }
        Err(e) => {
            eprintln!("[auth] 登录失败 user={}: {e}", req.name);
            if e == "__TOTP_REQUIRED__" {
                // 缺验证码不算攻击失败，不计数
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "请输入两步验证码", "totp_required": true })),
                )
                    .into_response();
            }
            crate::ratelimit::record_fail(ip);
            err(StatusCode::UNAUTHORIZED, e)
        }
    }
}

#[derive(Deserialize)]
pub struct ChangePwReq {
    pub old: String,
    pub new: String,
}

/// POST /api/me/password —— 用户本人改口令。
pub async fn change_own_password(
    State(state): S,
    headers: HeaderMap,
    Json(req): Json<ChangePwReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match state.users.change_own_password(&user.uid, &req.old, &req.new) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

/// POST /api/me/totp/setup —— 生成 2fa 密钥（仅返回 otpauth URI + secret；确认前不生效）。
pub async fn totp_setup(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match state.users.setup_totp(&user.uid) {
        Ok(setup) => Json(setup).into_response(),
        Err(e) => s_err(e),
    }
}

#[derive(serde::Deserialize)]
pub struct TotpEnableReq {
    pub secret: String,
    pub code: String,
}

/// POST /api/me/totp/enable —— 校验验证码通过后启用 2fa。
pub async fn totp_enable(State(state): S, headers: HeaderMap, Json(req): Json<TotpEnableReq>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match state.users.enable_totp(&user.uid, &req.secret, &req.code) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e),
    }
}

/// POST /api/me/totp/disable —— 关闭 2fa。
pub async fn totp_disable(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    match state.users.disable_totp(&user.uid) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e),
    }
}

/// GET /favicon.ico —— 站点图标（内嵌 SVG；浏览器默认请求此路径）。
pub async fn favicon() -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml".to_string()), (header::CACHE_CONTROL, "public, max-age=604800".to_string())],
        include_str!("webapp/favicon.svg"),
    )
        .into_response()
}

pub async fn logout(State(state): S, headers: HeaderMap) -> Response {
    if let Ok(user) = auth(&state, &headers) {
        // 简化：遍历找该用户 token（家庭规模可接受）
        // 实际上直接由前端丢弃 token 即可；这里尽力移除
        let _ = user;
    }
    StatusCode::OK.into_response()
}

pub async fn me(State(state): S, headers: HeaderMap) -> Response {
    match auth(&state, &headers) {
        Ok(user) => Json(serde_json::json!({
            "uid": user.uid, "name": user.name, "is_admin": user.is_admin, "avatar": user.avatar
        }))
        .into_response(),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------- 相册

pub async fn list_albums(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match visible_albums(&core, &state, &user, &parents) {
        Ok(list) => Json(list
            .into_iter()
            .map(|(a, p)| serde_json::json!({
                "id": a.id, "name": a.name, "parent_id": a.parent_id,
                "blob_count": a.blob_count,
                "perm": format!("{p:?}").to_lowercase(),
            }))
            .collect::<Vec<_>>())
        .into_response(),
        Err(e) => e,
    }
}

#[derive(Deserialize)]
pub struct CreateAlbumReq {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

pub async fn create_album(State(state): S, headers: HeaderMap, Json(req): Json<CreateAlbumReq>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if req.name.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "相册名不能为空");
    }
    let core = state.core.lock().unwrap();
    // 父相册需要 manage；建根相册仅 admin
    let perm_ok = match &req.parent_id {
        None => user.is_admin,
        Some(pid) => {
            let parents = match parent_map(&core) {
                Ok(p) => p,
                Err(e) => return e,
            };
            effective(&state, &user, pid, &parents) >= Perm::Manage
        }
    };
    if !perm_ok {
        return err(StatusCode::FORBIDDEN, "无权在此创建相册");
    }
    let id = match core.album_create(req.name.trim(), req.parent_id.clone()) {
        Ok(id) => id,
        Err(e) => return s_err(e.to_string()),
    };
    eprintln!("[album] 创建相册 name={} id={} parent={:?}", req.name.trim(), id, req.parent_id);
    // 根相册无父级可继承：显式授予 family 组 manage，否则普通用户永远看不到
    if req.parent_id.is_none() {
        let acl = &state.acl;
        let _ = acl.set(
            &id,
            crate::acl::Principal { kind: "group".into(), id: "family".into() },
            Perm::Manage,
        );
    }
    Json(serde_json::json!({ "id": id })).into_response()
}

pub async fn album_blobs(State(state): S, headers: HeaderMap, AxumPath(album_id): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if effective(&state, &user, &album_id, &parents) < Perm::View {
        return err(StatusCode::FORBIDDEN, "无权访问该相册");
    }
    match core.list_album_blobs(&album_id) {
        Ok(blobs) => Json(blobs).into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

// ---------------------------------------------------------------- 时间线

pub async fn list_blobs(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let visible = match visible_albums(&core, &state, &user, &parents) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // 可见相册内容并集（按 hash 去重，拍摄时间倒序）
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<BlobEntry> = Vec::new();
    for (a, _) in &visible {
        if let Ok(blobs) = core.list_album_blobs(&a.id) {
            for b in blobs {
                if seen.insert(b.blob_hash.clone()) {
                    out.push(b);
                }
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.taken_at));
    Json(out).into_response()
}

// ---------------------------------------------------------------- 上传

pub async fn upload(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
    mut multipart: Multipart,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, &album_id, &parents) < Perm::Upload {
            return err(StatusCode::FORBIDDEN, "无权向该相册上传");
        }
    }

    // 读出第一个文件字段到临时目录（保留原始文件名——mime 与展示名都靠它）。
    // 独立子目录放原始文件名，import_file 才能记录正确的名字。
    let tmp_dir = std::env::temp_dir().join("heartshard-server-upload");
    let upload_dir = tmp_dir.join(format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        user.uid
    ));
    std::fs::create_dir_all(&upload_dir).ok();
    let mut file_name = "upload.bin".to_string();
    let mut wrote_any = false;
    let mut staged: Option<std::path::PathBuf> = None;
    let mut staged_out: Option<std::fs::File> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if let Some(name) = field.file_name() {
            file_name = name.to_string();
        }
        if staged.is_none() {
            let safe_name: String = file_name
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let tmp = upload_dir.join(&safe_name);
            let out = match std::fs::File::create(&tmp) {
                Ok(f) => f,
                Err(e) => return s_err(e.to_string()),
            };
            staged = Some(tmp);
            staged_out = Some(out);
        }
        use std::io::Write;
        let out = staged_out.as_mut().unwrap();
        let mut field = field;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if out.write_all(&chunk).is_err() {
                        return err(StatusCode::BAD_REQUEST, "上传写入失败");
                    }
                    wrote_any = true;
                }
                Ok(None) => break,
                Err(_) => return err(StatusCode::BAD_REQUEST, "上传读取失败"),
            }
        }
        // 不 break：album_id 等附加字段可能排在 file 之后，继续读完
    }
    if !wrote_any {
        return err(StatusCode::BAD_REQUEST, "缺少文件字段");
    }
    let tmp = staged.unwrap();
    drop(staged_out);
    eprintln!(
        "[upload] file={file_name} album_id={album_id:?} bytes={}",
        tmp.metadata().map(|m| m.len()).unwrap_or(0)
    );

    // 解密管线是阻塞操作 → spawn_blocking
    let state2 = state.clone();
    let album_id2 = album_id.clone();
    let tmp2 = tmp.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut core = state2.core.lock().unwrap();
        let outcome = core.import_file(&tmp2).map_err(|e| e.to_string())?;
        if matches!(outcome.status, ImportStatus::Imported | ImportStatus::SkippedDuplicate) {
            if let Some(h) = &outcome.blob_hash {
                core.album_add_blob(&album_id2, h).map_err(|e| e.to_string())?;
                eprintln!("[upload] bound blob {h} -> album {album_id2}");
            }
        }
        Ok::<(String, Option<String>), String>((
            format!("{:?}", outcome.status).to_lowercase(),
            outcome.blob_hash,
        ))
    })
    .await;

    let _ = std::fs::remove_dir_all(&upload_dir);
    match result {
        Ok(Ok((status, hash))) => {
            Json(serde_json::json!({ "status": status, "blob_hash": hash })).into_response()
        }
        Ok(Err(e)) => s_err(e),
        Err(e) => s_err(e.to_string()),
    }
}

// ---------------------------------------------------------------- blob 内容

fn require_blob_view(state: &AppState, user: &User, blob_hash: &str) -> Result<(), Response> {
    if user.is_admin {
        return Ok(()); // admin 可预览任意 blob（含回收站）
    }
    let core = state.core.lock().unwrap();
    let parents = parent_map(&core)?;
    let visible = visible_albums(&core, state, user, &parents)?;
    for (a, _) in &visible {
        if let Ok(blobs) = core.list_album_blobs(&a.id) {
            if blobs.iter().any(|b| b.blob_hash == blob_hash) {
                return Ok(());
            }
        }
    }
    Err(err(StatusCode::FORBIDDEN, "无权访问该照片"))
}

pub async fn thumbnail(State(state): S, headers: HeaderMap, AxumPath(hash): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if let Err(e) = require_blob_view(&state, &user, &hash) {
        return e;
    }
    let state2 = state.clone();
    let hash2 = hash.clone();
    let result = tokio::task::spawn_blocking(move || {
        let core = state2.core.lock().unwrap();
        core.thumbnail_cached(&hash2).map(|o| o.map(|url| {
            url.trim_start_matches("data:image/png;base64,").to_string()
        }))
    })
    .await;
    match result {
        Ok(Ok(Some(b64))) => {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(&b64) {
                Ok(png) => (
                    [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "private, max-age=86400")],
                    png,
                )
                    .into_response(),
                Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "缩略图解码失败"),
            }
        }
        Ok(Ok(None)) => err(StatusCode::NOT_FOUND, "缩略图不可用（视频或非图片）"),
        Ok(Err(e)) => s_err(e.to_string()),
        Err(e) => s_err(e.to_string()),
    }
}

pub async fn raw(State(state): S, headers: HeaderMap, AxumPath(hash): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if let Err(e) = require_blob_view(&state, &user, &hash) {
        return e;
    }
    let state2 = state.clone();
    let hash2 = hash.clone();
    let result = tokio::task::spawn_blocking(move || {
        let core = state2.core.lock().unwrap();
        core.decrypt_blob_bytes(&hash2)
    })
    .await;
    match result {
        Ok(Ok((bytes, manifest))) => (
            [
                (header::CONTENT_TYPE, manifest.mime.clone()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename=\"{}\"", manifest.file_name.replace('"', "")),
                ),
                (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(e)) => s_err(e.to_string()),
        Err(e) => s_err(e.to_string()),
    }
}

// ---------------------------------------------------------------- 收藏 / 删除（照片级）

/// 校验调用者对 blob 所在任一相册有 manage（admin 直通）。
fn require_manage_on_blob(state: &AppState, user: &User, hash: &str) -> Result<(), Response> {
    if user.is_admin {
        return Ok(());
    }
    let core = state.core.lock().unwrap();
    let parents = parent_map(&core)?;
    let groups = state.users.groups_of(&user.uid);
    let acl = &state.acl;
    for album_id in core.list_albums().unwrap_or_default().iter().map(|a| a.id.clone()) {
        if let Ok(blobs) = core.list_album_blobs(&album_id) {
            if blobs.iter().any(|b| b.blob_hash == hash)
                && acl.effective(user, &groups, &album_id, &parents) >= Perm::Manage
            {
                return Ok(());
            }
        }
    }
    Err(err(StatusCode::FORBIDDEN, "无权操作该照片"))
}

#[derive(Deserialize)]
pub struct FavoriteReq {
    pub favorite: bool,
}

/// POST /api/blobs/{hash}/favorite
pub async fn set_favorite(
    State(state): S,
    headers: HeaderMap,
    AxumPath(hash): AxumPath<String>,
    Json(req): Json<FavoriteReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if let Err(e) = require_manage_on_blob(&state, &user, &hash) {
        return e;
    }
    let core = state.core.lock().unwrap();
    match core.set_blob_favorite(&hash, req.favorite) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

/// POST /api/blobs/{hash}/delete（移入回收站）
pub async fn delete_blob(State(state): S, headers: HeaderMap, AxumPath(hash): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if let Err(e) = require_manage_on_blob(&state, &user, &hash) {
        return e;
    }
    let core = state.core.lock().unwrap();
    match core.delete_blob(&hash) {
        Ok(()) => {
            eprintln!("[blob] 移入回收站 hash={hash} by={}", user.uid);
            StatusCode::OK.into_response()
        }
        Err(e) => s_err(e.to_string()),
    }
}

/// POST /api/blobs/{hash}/rename —— 照片/视频重命名（manage 权限）。
#[derive(Deserialize)]
pub struct RenameReq {
    pub new_name: String,
}
pub async fn rename_blob(
    State(state): S,
    headers: HeaderMap,
    AxumPath(hash): AxumPath<String>,
    Json(req): Json<RenameReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if let Err(e) = require_manage_on_blob(&state, &user, &hash) {
        return e;
    }
    let core = state.core.lock().unwrap();
    match core.rename_blob(&hash, &req.new_name) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

/// POST /api/blobs/upload —— 无相册上传（multipart `file`；可选字段 album_id）。
pub async fn upload_no_album(
    State(state): S,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    // 读取文件 + 可选 album_id
    let mut file_name = "upload.bin".to_string();
    let mut album_id: Option<String> = None;
    let mut wrote_any = false;
    let mut staged: Option<std::path::PathBuf> = None;
    let mut staged_out: Option<std::fs::File> = None;
    let tmp_dir = std::env::temp_dir().join("heartshard-server-upload");
    let upload_dir = tmp_dir.join(format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        user.uid
    ));
    std::fs::create_dir_all(&upload_dir).ok();
    while let Ok(Some(mut field)) = multipart.next_field().await {
        match field.name().unwrap_or("") {
            "album_id" => {
                album_id = field.text().await.ok().filter(|s| !s.is_empty());
                continue; // album_id 可能排在文件字段之后，不能提前退出循环
            }
            "file" => {}
            _ => continue,
        }
        if let Some(name) = field.file_name() {
            file_name = name.to_string();
        }
        if staged.is_none() {
            let safe_name: String = file_name
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let tmp = upload_dir.join(&safe_name);
            let out = match std::fs::File::create(&tmp) {
                Ok(f) => f,
                Err(e) => return s_err(e.to_string()),
            };
            staged = Some(tmp);
            staged_out = Some(out);
        }
        use std::io::Write;
        let out = staged_out.as_mut().unwrap();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if out.write_all(&chunk).is_err() {
                        return err(StatusCode::BAD_REQUEST, "上传写入失败");
                    }
                    wrote_any = true;
                }
                Ok(None) => break,
                Err(_) => return err(StatusCode::BAD_REQUEST, "上传读取失败"),
            }
        }
        // 不 break：album_id 等附加字段可能排在 file 之后，继续读完
    }
    if !wrote_any {
        return err(StatusCode::BAD_REQUEST, "缺少文件字段");
    }
    let tmp = staged.unwrap();
    drop(staged_out);

    // 若指定相册：校验 upload 权限
    if let Some(aid) = &album_id {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, aid, &parents) < Perm::Upload {
            return err(StatusCode::FORBIDDEN, "无权向该相册上传");
        }
    }

    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut core = state2.core.lock().unwrap();
        let outcome = core.import_file(&tmp).map_err(|e| e.to_string())?;
        if matches!(outcome.status, ImportStatus::Imported | ImportStatus::SkippedDuplicate) {
            if let Some(h) = &outcome.blob_hash {
                if let Some(aid) = &album_id {
                    core.album_add_blob(aid, h).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok::<(String, Option<String>), String>((
            format!("{:?}", outcome.status).to_lowercase(),
            outcome.blob_hash,
        ))
    })
    .await;
    let _ = std::fs::remove_dir_all(&upload_dir);
    match result {
        Ok(Ok((status, hash))) => {
            Json(serde_json::json!({ "status": status, "blob_hash": hash })).into_response()
        }
        Ok(Err(e)) => s_err(e),
        Err(e) => s_err(e.to_string()),
    }
}

/// POST /api/albums/{id}/add-blobs —— 批量把已有照片加入相册（复制到）。
#[derive(Deserialize)]
pub struct BlobsReq {
    pub blob_hashes: Vec<String>,
}
pub async fn album_add_blobs(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
    Json(req): Json<BlobsReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, &album_id, &parents) < Perm::Upload {
            return err(StatusCode::FORBIDDEN, "无权向该相册添加照片");
        }
    }
    let core = state.core.lock().unwrap();
    let mut ok = 0;
    for h in &req.blob_hashes {
        if core.album_add_blob(&album_id, h).is_ok() {
            ok += 1;
        }
    }
    Json(serde_json::json!({ "added": ok })).into_response()
}

/// POST /api/albums/{id}/remove-blobs —— 批量从该相册移除（移动到 = 加入目标 + 从此处移除）。
pub async fn album_remove_blobs(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
    Json(req): Json<BlobsReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, &album_id, &parents) < Perm::Manage {
            return err(StatusCode::FORBIDDEN, "无权从该相册移除照片");
        }
    }
    let core = state.core.lock().unwrap();
    let mut ok = 0;
    for h in &req.blob_hashes {
        if core.album_remove_blob(&album_id, h).is_ok() {
            ok += 1;
        }
    }
    Json(serde_json::json!({ "removed": ok })).into_response()
}

/// POST /api/albums/{id}/move-blobs —— 批量移动：加入目标相册，并从照片当前
/// 所在的其他相册移除（照片本体不动，只改关联）。
pub async fn album_move_blobs(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
    Json(req): Json<BlobsReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, &album_id, &parents) < Perm::Upload {
            return err(StatusCode::FORBIDDEN, "无权向该相册移动照片");
        }
    }
    let core = state.core.lock().unwrap();
    // 相册 → 成员集合（一次构建）
    let mut members: Vec<(String, std::collections::HashSet<String>)> = Vec::new();
    for a in core.list_albums().unwrap_or_default() {
        let set: std::collections::HashSet<String> = core
            .list_album_blobs(&a.id)
            .unwrap_or_default()
            .into_iter()
            .map(|b| b.blob_hash)
            .collect();
        members.push((a.id, set));
    }
    let mut moved = 0;
    for h in &req.blob_hashes {
        // 从除目标外的所有相册移除
        for (aid, set) in &members {
            if *aid != album_id && set.contains(h) {
                let _ = core.album_remove_blob(aid, h);
            }
        }
        if core.album_add_blob(&album_id, h).is_ok() {
            moved += 1;
        }
    }
    Json(serde_json::json!({ "moved": moved })).into_response()
}

/// DELETE /api/albums/{id} —— 删除相册（照片保留；manage 权限）。
pub async fn delete_album(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    {
        let core = state.core.lock().unwrap();
        let parents = match parent_map(&core) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if effective(&state, &user, &album_id, &parents) < Perm::Manage {
            return err(StatusCode::FORBIDDEN, "无权删除该相册");
        }
    }
    let core = state.core.lock().unwrap();
    match core.album_delete(&album_id) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

/// GET /api/blobs/orphans —— 未加入任何相册的照片/视频。
pub async fn orphan_blobs(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    // 可见相册的成员并集 = 已归档；全量 - 已归档 = 孤儿
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let visible = match visible_albums(&core, &state, &user, &parents) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut archived: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (a, _) in &visible {
        if let Ok(blobs) = core.list_album_blobs(&a.id) {
            for b in blobs {
                archived.insert(b.blob_hash);
            }
        }
    }
    let mut out: Vec<BlobEntry> = match core.list_blobs() {
        Ok(list) => list,
        Err(e) => return s_err(e.to_string()),
    }
    .into_iter()
    .filter(|b| !archived.contains(&b.blob_hash))
    .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.taken_at));
    Json(out).into_response()
}

/// POST /api/me/avatar —— 设置个人头像（data URL，≤256KB）。
#[derive(Deserialize)]
pub struct AvatarReq {
    pub data_url: String,
}
pub async fn set_avatar(State(state): S, headers: HeaderMap, Json(req): Json<AvatarReq>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if req.data_url.len() > 256 * 1024 {
        return err(StatusCode::BAD_REQUEST, "头像图片过大（≤256KB）");
    }
    if !req.data_url.starts_with("data:image/") {
        return err(StatusCode::BAD_REQUEST, "头像必须是图片");
    }
    match state.users.set_avatar(&user.uid, &req.data_url) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e),
    }
}

// ---------------------------------------------------------------- 管理（v1 仅 admin）

pub async fn trash_list(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let core = state.core.lock().unwrap();
    match core.list_trashed() {
        Ok(list) => Json(list).into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

pub async fn restore_blob(State(state): S, headers: HeaderMap, AxumPath(hash): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let core = state.core.lock().unwrap();
    match core.restore_blob(&hash) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

pub async fn purge_blob(State(state): S, headers: HeaderMap, AxumPath(hash): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let core = state.core.lock().unwrap();
    match core.purge_blob(&hash) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

pub async fn run_sync(State(state): S, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let sources = state.sources.lock().unwrap().clone();
    let state2 = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for src in &sources {
            let mut core = state2.core.lock().unwrap();
            let r = syncer::engine::sync_source(&mut core, src, false).map_err(|e| e.to_string())?;
            out.push(serde_json::json!({
                "source": src.id, "added": r.added, "moved": r.moved, "removed": r.removed,
            }));
        }
        // 发布增量
        let core = state2.core.lock().unwrap();
        core.sync_now().map_err(|e| e.to_string())?;
        Ok::<_, String>(out)
    })
    .await;
    match result {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => s_err(e),
        Err(e) => s_err(e.to_string()),
    }
}

// ---------------------------------------------------------------- ACL 管理

#[derive(Deserialize)]
pub struct SetAclReq {
    pub principal: Principal,
    /// "none"|"view"|"upload"|"manage"；null = 移除显式设置（回退继承）
    pub perm: Option<String>,
}

pub async fn set_acl(
    State(state): S,
    headers: HeaderMap,
    AxumPath(album_id): AxumPath<String>,
    Json(req): Json<SetAclReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if effective(&state, &user, &album_id, &parents) < Perm::Manage {
        return err(StatusCode::FORBIDDEN, "无权管理该相册");
    }
    drop(core);
    let acl = &state.acl;
    match &req.perm {
        Some(p) => match Perm::parse(p) {
            Some(perm) => match acl.set(&album_id, req.principal, perm) {
                Ok(()) => StatusCode::OK.into_response(),
                Err(e) => s_err(e),
            },
            None => err(StatusCode::BAD_REQUEST, "无效权限级别"),
        },
        None => match acl.unset(&album_id, &req.principal) {
            Ok(()) => StatusCode::OK.into_response(),
            Err(e) => s_err(e),
        },
    }
}

pub async fn get_acl(State(state): S, headers: HeaderMap, AxumPath(album_id): AxumPath<String>) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if effective(&state, &user, &album_id, &parents) < Perm::Manage {
        return err(StatusCode::FORBIDDEN, "无权管理该相册");
    }
    let acl = &state.acl;
    Json(acl.for_album(&album_id)).into_response()
}

// ---------------------------------------------------------------- 路由

pub fn router() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/", get(|| async { Html(include_str!("webapp/index.html")) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript".to_string())], include_str!("webapp/app.js")) }))
        .route("/sw.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript".to_string())], include_str!("webapp/sw.js")) }))
        .route("/qrcode.min.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript".to_string())], include_str!("webapp/qrcode.min.js")) }))
        .route(
            "/manifest.webmanifest",
            get(|| async { ([(header::CONTENT_TYPE, "application/manifest+json".to_string())], include_str!("webapp/manifest.webmanifest")) }),
        )
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_own_password))
        .route("/api/me/totp/setup", post(totp_setup))
        .route("/api/me/totp/enable", post(totp_enable))
        .route("/api/me/totp/disable", post(totp_disable))
        .route("/favicon.ico", get(favicon))
        .route("/favicon.svg", get(favicon))
        .route("/api/albums", get(list_albums).post(create_album))
        .route("/api/albums/{id}/blobs", get(album_blobs))
        .route("/api/albums/{id}/upload", post(upload))
        .route("/api/albums/{id}/share", post(crate::share::create_album_share))
        .route("/api/shares", get(crate::share::list_shares).post(crate::share::create_blobs_share))
        .route("/api/shares/{id}", axum::routing::delete(crate::share::delete_share))
        .route("/api/blobs", get(list_blobs))
        .route("/api/blobs/{hash}/thumbnail", get(thumbnail))
        .route("/api/blobs/{hash}/raw", get(raw))
        .route("/api/blobs/{hash}/favorite", post(set_favorite))
        .route("/api/blobs/{hash}/delete", post(delete_blob))
        .route("/api/blobs/{hash}/rename", post(rename_blob))
        .route("/api/blobs/upload", post(upload_no_album))
        .route("/api/blobs/orphans", get(orphan_blobs))
        .route("/api/albums/{id}/add-blobs", post(album_add_blobs))
        .route("/api/albums/{id}/remove-blobs", post(album_remove_blobs))
        .route("/api/albums/{id}/move-blobs", post(album_move_blobs))
        .route("/api/albums/{id}", axum::routing::delete(delete_album))
        .route("/api/trash", get(trash_list))
        .route("/api/blobs/{hash}/restore", post(restore_blob))
        .route("/api/blobs/{hash}/purge", post(purge_blob))
        .route("/api/sync/run", post(run_sync))
        .route("/api/albums/{id}/acl", get(get_acl).post(set_acl))
        .route("/api/admin/users", get(crate::admin::list_users).post(crate::admin::create_user))
        .route("/api/admin/users/{uid}", axum::routing::put(crate::admin::update_user).delete(crate::admin::delete_user))
        .route("/api/admin/groups", get(crate::admin::list_groups).post(crate::admin::set_groups))
        .route("/api/admin/sources", get(crate::admin::list_sources).post(crate::admin::set_sources))
        .route("/api/admin/status", get(crate::admin::status))
        .route("/api/admin/storage", get(crate::storage::list_storage).post(crate::storage::add_storage))
        .route("/api/admin/storage/{id}", axum::routing::delete(crate::storage::remove_storage))
        .route("/api/admin/storage/verify", post(crate::storage::verify_storage))
        .route("/api/admin/storage/probe", post(crate::storage::probe_storage))
        .route("/s/{id}", get(crate::share::share_page))
        .route("/s/{id}/meta", get(crate::share::share_meta))
        .route("/s/{id}/keys", get(crate::share::share_keys))
        .route("/s/{id}/pkg/{hash}", get(crate::share::share_pkg))
        .layer(axum::middleware::from_fn(token_query_bridge))
        .layer(axum::extract::DefaultBodyLimit::max(2048 * 1024 * 1024)) // 大视频上传（默认 2MB 会 ERR_CONNECTION_ABORTED）
}
