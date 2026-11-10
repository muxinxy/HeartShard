//! 管理后台 API（M5a→M5b''）：用户/组/同步源的网页端管理，配置全部存元数据库
//! （sqlite 默认 / mysql 可选），server.toml 仅负责引导。

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
    match state.users.list_users() {
        Ok(users) => Json(users).into_response(),
        Err(e) => s_err(e),
    }
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    match state.users.add_user(&req.name, &req.password, req.admin) {
        Ok(u) => Json(u).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
pub struct UpdateUserReq {
    pub password: Option<String>,
    pub name: Option<String>,
    pub admin: Option<bool>,
    pub enabled: Option<bool>,
    pub avatar: Option<String>,
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
    match state.users.update_user(
        &uid,
        req.password.as_deref(),
        req.name.as_deref(),
        req.admin,
        req.enabled,
        req.avatar.as_deref(),
    ) {
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
    match state.users.remove_user(&uid) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---------------- 组管理 ----------------

pub async fn list_groups(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Err(e) = require_admin(&state, &headers) {
        return e;
    }
    match state.users.list_groups() {
        Ok(groups) => Json(groups).into_response(),
        Err(e) => s_err(e),
    }
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
    match state.users.set_groups(req.groups) {
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
    let json_values: Vec<serde_json::Value> = sources
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect();
    if let Err(e) = state.users.meta_handle().save_sources(&json_values) {
        return s_err(e);
    }
    *state.sources.lock().unwrap() = sources;
    StatusCode::OK.into_response()
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
    let users = state.users.list_users().unwrap_or_default();
    let groups = state.users.list_groups().unwrap_or_default();
    Json(serde_json::json!({
        "users": users.len(),
        "groups": groups.len(),
        "sources": state.sources.lock().unwrap().len(),
        "shares": state.shares.all().len(),
        "albums": albums,
        "blobs": blobs,
        "trash": trash,
        "sync_interval_secs": state.cfg.sync_interval_secs,
    }))
    .into_response()
}
