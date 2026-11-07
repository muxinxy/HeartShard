//! 管理后台 API（M5a）：用户/组/同步源的网页端管理，配置全部运行时化
//! （users.json / groups.json / sources.json），server.toml 仅负责引导。

use crate::api::{auth, err, s_err};
use crate::auth::Group;
use crate::AppState;
use axum::{
    extract::{Path as AxumPathT, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<crate::auth::User, Response> {
    let user = auth(state, headers)?;
    if !user.is_admin {
        return Err(err(StatusCode::FORBIDDEN, "仅管理员"));
    }
    Ok(user)
}

// ---------------- 用户管理 ----------------

#[derive(Deserialize)]
pub struct CreateUserReq {
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub admin: bool,
}

pub async fn list_users(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    let users = state.users.lock().unwrap().users.clone();
    Json(users).into_response()
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    match state.users.lock().unwrap().add_user(&req.name, &req.password, req.admin) {
        Ok(u) => Json(u).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
pub struct UpdateUserReq {
    pub password: Option<String>,
    pub name: Option<String>,
    pub admin: Option<bool>,
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPathT(uid): AxumPathT<String>,
    Json(req): Json<UpdateUserReq>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    match state
        .users
        .lock()
        .unwrap()
        .update_user(&uid, req.password.as_deref(), req.name.as_deref(), req.admin)
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPathT(uid): AxumPathT<String>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    match state.users.lock().unwrap().remove_user(&uid) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---------------- 组管理 ----------------

pub async fn list_groups(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    let groups = state.users.lock().unwrap().groups.clone();
    Json(groups).into_response()
}

#[derive(Deserialize)]
pub struct SetGroupsReq {
    pub groups: Vec<Group>,
}

/// 整体保存组列表（管理界面直接编辑成员）。
pub async fn set_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SetGroupsReq>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    // family 组保留（全员组由启动时重建，这里允许编辑其他组）
    match state.users.lock().unwrap().set_groups(req.groups) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => s_err(e),
    }
}

// ---------------- 同步源管理 ----------------

pub async fn list_sources(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    Json(state.sources.lock().unwrap().clone()).into_response()
}

pub async fn set_sources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(sources): Json<Vec<syncer::SourceConfig>>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    // 校验 id 唯一
    let mut ids = std::collections::HashSet::new();
    for s in &sources {
        if !ids.insert(s.id.clone()) {
            return err(StatusCode::BAD_REQUEST, format!("源 id 重复: {}", s.id));
        }
        if s.kind != "local" && s.kind != "s3" && s.kind != "webdav" && s.kind != "openlist" {
            return err(StatusCode::BAD_REQUEST, format!("未知源类型: {}", s.kind));
        }
    }
    *state.sources.lock().unwrap() = sources;
    if let Err(e) = save_sources(&state) {
        return s_err(e);
    }
    StatusCode::OK.into_response()
}

pub fn load_sources(root: &std::path::Path) -> Vec<syncer::SourceConfig> {
    std::fs::read(root.join("sources.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) fn save_sources(state: &AppState) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(&state.sources.lock().unwrap().clone())
        .map_err(|e| e.to_string())?;
    let path = state.server_root.join("sources.json");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

// ---------------- 状态总览 ----------------

pub async fn status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    let core = state.core.lock().unwrap();
    let blobs = core.list_blobs().map(|b| b.len()).unwrap_or(0);
    let trash = core.list_trashed().map(|b| b.len()).unwrap_or(0);
    let albums = core.list_albums().map(|a| a.len()).unwrap_or(0);
    drop(core);
    let users_store = state.users.lock().unwrap();
    Json(serde_json::json!({
        "users": users_store.users.len(),
        "groups": users_store.groups.len(),
        "sources": state.sources.lock().unwrap().len(),
        "shares": state.shares.lock().unwrap().shares.len(),
        "albums": albums,
        "blobs": blobs,
        "trash": trash,
        "sync_interval_secs": state.cfg.sync_interval_secs,
    }))
    .into_response()
}
