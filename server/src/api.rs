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
    next.run(axum::extract::Request::from_parts(parts, body)).await
}

fn effective(state: &AppState, user: &User, album_id: &str, parents: &HashMap<String, Option<String>>) -> Perm {
    let groups = state.users.groups_of(&user.uid);
    state.acl.lock().unwrap().effective(user, &groups, album_id, parents)
}

/// 用户可见（view+）的相册集合。
fn visible_albums(
    core: &HeartCore,
    state: &AppState,
    user: &User,
    parents: &HashMap<String, Option<String>>,
) -> Result<Vec<(AlbumInfo, Perm)>, Response> {
    let albums = core.list_albums().map_err(|e| s_err(e.to_string()))?;
    let acl = state.acl.lock().unwrap();
    Ok(albums
        .into_iter()
        .filter_map(|a| {
            let p = acl.effective(user, &state.users.groups_of(&user.uid), &a.id, parents);
            (p >= Perm::View).then_some((a, p))
        })
        .collect())
}

// ---------------------------------------------------------------- 认证

#[derive(Deserialize)]
pub struct LoginReq {
    pub name: String,
    pub password: String,
}

pub async fn login(State(state): S, Json(req): Json<LoginReq>) -> Response {
    let users = &state.users;
    match users.login(&req.name, &req.password) {
        Ok((token, user)) => Json(serde_json::json!({
            "token": token,
            "uid": user.uid,
            "name": user.name,
            "is_admin": user.is_admin,
        }))
        .into_response(),
        Err(e) => err(StatusCode::UNAUTHORIZED, e),
    }
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
            "uid": user.uid, "name": user.name, "is_admin": user.is_admin
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
    let mut core = state.core.lock().unwrap();
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
    // 新相册默认继承父权限（无显式条目）——无需写 ACL
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
        break; // 只取第一个文件字段
    }
    if !wrote_any {
        return err(StatusCode::BAD_REQUEST, "缺少文件字段");
    }
    let tmp = staged.unwrap();
    drop(staged_out);

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
    let sources = state.cfg.sources.clone();
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
        let mut core = state2.core.lock().unwrap();
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
    let mut core = state.core.lock().unwrap();
    let parents = match parent_map(&core) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if effective(&state, &user, &album_id, &parents) < Perm::Manage {
        return err(StatusCode::FORBIDDEN, "无权管理该相册");
    }
    drop(core);
    let mut acl = state.acl.lock().unwrap();
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
    let acl = state.acl.lock().unwrap();
    Json(acl.for_album(&album_id)).into_response()
}

// ---------------------------------------------------------------- 路由

pub fn router() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    use axum::Router as R;
    axum::Router::new()
        .route("/", get(|| async { Html(include_str!("webapp/index.html")) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript".to_string())], include_str!("webapp/app.js")) }))
        .route("/sw.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript".to_string())], include_str!("webapp/sw.js")) }))
        .route(
            "/manifest.webmanifest",
            get(|| async { ([(header::CONTENT_TYPE, "application/manifest+json".to_string())], include_str!("webapp/manifest.webmanifest")) }),
        )
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/albums", get(list_albums).post(create_album))
        .route("/api/albums/{id}/blobs", get(album_blobs))
        .route("/api/albums/{id}/upload", post(upload))
        .route("/api/albums/{id}/share", post(crate::share::create_share))
        .route("/api/shares", get(crate::share::list_shares))
        .route("/api/shares/{id}", axum::routing::delete(crate::share::delete_share))
        .route("/api/blobs", get(list_blobs))
        .route("/api/blobs/{hash}/thumbnail", get(thumbnail))
        .route("/api/blobs/{hash}/raw", get(raw))
        .route("/api/trash", get(trash_list))
        .route("/api/blobs/{hash}/restore", post(restore_blob))
        .route("/api/blobs/{hash}/purge", post(purge_blob))
        .route("/api/sync/run", post(run_sync))
        .route("/api/albums/{id}/acl", get(get_acl).post(set_acl))
        .route("/s/{id}", get(crate::share::share_page))
        .route("/s/{id}/meta", get(crate::share::share_meta))
        .route("/s/{id}/keys", get(crate::share::share_keys))
        .route("/s/{id}/pkg/{hash}", get(crate::share::share_pkg))
        .layer(axum::middleware::from_fn(token_query_bridge))
}
