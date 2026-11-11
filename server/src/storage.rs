//! 存储管理 API（M5b'）：网页端增删云存储（经 HeartCore 的加密注册表落盘，
//! 凭据不落后台明文）。仅 admin。

use crate::api::{auth, err, s_err};
use crate::AppState;
use axum::{
    extract::{Path as AxumPathT, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

pub async fn list_storage(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let core = state.core.lock().unwrap();
    match core.storage_status() {
        Ok(list) => Json(list).into_response(),
        Err(e) => s_err(e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct AddStorageReq {
    /// local | s3 | webdav | openlist
    pub kind: String,
    pub id: Option<String>,
    pub root: Option<String>,
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
    pub path_style: Option<bool>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
    pub base_path: Option<String>,
}

pub async fn add_storage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AddStorageReq>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let mut core = state.core.lock().unwrap();
    let result: Result<String, heart_core::library::LibError> = match req.kind.as_str() {
        "local" => {
            let root = req.root.clone().unwrap_or_default();
            let id = req.id.clone().unwrap_or_else(|| {
                root.trim_end_matches(['\\', '/'])
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or("local")
                    .to_string()
            });
            if root.is_empty() {
                return err(StatusCode::BAD_REQUEST, "本地目录不能为空");
            }
            core.add_storage_local(&id, &root)
        }
        "s3" => {
            use heart_core::storage_s3::S3Config;
            let cfg = S3Config {
                endpoint: req.endpoint.clone().unwrap_or_default(),
                bucket: req.bucket.clone().unwrap_or_default(),
                access_key: req.access_key.clone().unwrap_or_default(),
                secret_key: req.secret_key.clone().unwrap_or_default(),
                region: req.region.clone().unwrap_or_else(|| "us-east-1".into()),
                path_style: req.path_style.unwrap_or(true),
            };
            if cfg.endpoint.is_empty() || cfg.bucket.is_empty() {
                return err(StatusCode::BAD_REQUEST, "S3 endpoint/bucket 不能为空");
            }
            core.add_storage_s3(cfg)
        }
        "webdav" => {
            use heart_core::storage_webdav::WebDavConfig;
            core.add_storage_webdav(WebDavConfig {
                base_url: req.endpoint.clone().unwrap_or_default(),
                username: req.username.clone().unwrap_or_default(),
                password: req.password.clone().unwrap_or_default(),
            })
        }
        "openlist" => {
            use heart_core::storage_openlist::OpenListConfig;
            core.add_storage_openlist(OpenListConfig {
                base_url: req.endpoint.clone().unwrap_or_default(),
                token: req.token.clone().unwrap_or_default(),
                username: req.username.clone().unwrap_or_default(),
                password: req.password.clone().unwrap_or_default(),
                base_path: req.base_path.clone().unwrap_or_else(|| "/heartshard".into()),
            })
        }
        other => return err(StatusCode::BAD_REQUEST, format!("未知存储类型: {other}")),
    };
    match result {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

pub async fn remove_storage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPathT(id): AxumPathT<String>,
) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let mut core = state.core.lock().unwrap();
    match core.remove_storage(&id) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

/// 连通性验证：对每个 Storage put/get/delete 探针对象。
pub async fn verify_storage(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let user = match auth(&state, &headers) {
        Ok(u) => u,
        Err(e) => return e,
    };
    if !user.is_admin {
        return err(StatusCode::FORBIDDEN, "仅管理员");
    }
    let core = state.core.lock().unwrap();
    let mut out = Vec::new();
    for s in core.storage_status().unwrap_or_default() {
        let ok = core.sync_probe(&s.id);
        out.push(serde_json::json!({
            "id": s.id, "kind": s.kind,
            "ok": ok.is_ok(),
            "error": ok.err().map(|e| e.to_string()),
        }));
    }
    Json(out).into_response()
}
