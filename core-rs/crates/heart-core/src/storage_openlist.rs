//! OpenList 适配器（M4；OpenList = AList 社区延续版）。
//!
//! API（与 AList 同形，以实测为准——简报 §14 与 §9.3 OpenList 注意事项）：
//! - 登录：`POST /api/auth/login` `{username,password}` → `{code:200,data:{token}}`
//! - 上传：`PUT /api/fs/put`，头 `Authorization: <token>`（**不加 Bearer 前缀**）、
//!   `File-Path: <url-encoded 全路径>`、`Content-Length`，body 为文件内容
//! - 下载：`GET /d/<path>`（原始下载端点，带 Authorization）
//! - 列目录：`POST /api/fs/list` `{path, page:1, per_page:0}` → `content[]{name,is_dir}`
//! - 删除：`POST /api/fs/remove` `{dir, names:[...]}`
//!
//! 验证：mock 服务器集成测试覆盖全部端点；真实 OpenList 实例验收
//! 与 s3-verify 同构（复用本适配器即可）。

use std::sync::Mutex;

use index::store::{BlobStore, StoreError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenListConfig {
    /// 例如 `http://127.0.0.1:5244`
    pub base_url: String,
    pub username: String,
    pub password: String,
    /// 挂载的基础目录（如 `/heartshard`），对象键附加在其后。
    pub base_path: String,
}

pub struct OpenListStore {
    config: OpenListConfig,
    client: reqwest::blocking::Client,
    token: Mutex<Option<String>>,
}

#[derive(Deserialize)]
struct OpenListResp<T> {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<T>,
}

#[derive(Deserialize, Default)]
struct LoginData {
    #[serde(default)]
    token: String,
}

#[derive(Serialize)]
struct LoginReq {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct ListEntry {
    name: String,
    #[serde(default)]
    is_dir: bool,
}

#[derive(Deserialize, Default)]
struct ListData {
    #[serde(default)]
    content: Vec<ListEntry>,
}

impl OpenListStore {
    pub fn new(config: OpenListConfig) -> Result<Self, StoreError> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| StoreError::Protocol(format!("http client: {e}")))?;
        Ok(Self { config, client, token: Mutex::new(None) })
    }

    fn full_path(&self, remote_key: &str) -> String {
        let base = self.config.base_path.trim_end_matches('/');
        format!("{base}/{remote_key}")
    }

    fn base(&self) -> &str {
        self.config.base_url.trim_end_matches('/')
    }

    /// 登录并缓存 token；已缓存则复用。
    fn ensure_token(&self) -> Result<String, StoreError> {
        if let Some(t) = self.token.lock().unwrap().as_ref() {
            return Ok(t.clone());
        }
        self.login()
    }

    fn login(&self) -> Result<String, StoreError> {
        let resp = self
            .client
            .post(format!("{}/api/auth/login", self.base()))
            .json(&LoginReq {
                username: self.config.username.clone(),
                password: self.config.password.clone(),
            })
            .send()
            .map_err(|e| StoreError::Protocol(format!("login: {e}")))?;
        let parsed: OpenListResp<LoginData> =
            resp.json().map_err(|e| StoreError::Protocol(format!("login body: {e}")))?;
        if parsed.code != 200 {
            return Err(StoreError::Protocol(format!(
                "login: code {}: {}",
                parsed.code, parsed.message
            )));
        }
        let token = parsed
            .data
            .ok_or_else(|| StoreError::Protocol("login: empty data".into()))?
            .token;
        *self.token.lock().unwrap() = Some(token.clone());
        Ok(token)
    }

    /// 带鉴权请求；401 时重新登录一次重试。
    fn authed(
        &self,
        method: reqwest::Method,
        url: String,
        json_body: Option<serde_json::Value>,
    ) -> Result<reqwest::blocking::Response, StoreError> {
        for attempt in 0..2 {
            let token = self.ensure_token()?;
            let mut req = self
                .client
                .request(method.clone(), &url)
                .header("Authorization", &token);
            if let Some(body) = &json_body {
                req = req.json(body);
            }
            let resp = req.send().map_err(|e| StoreError::Protocol(format!("{method}: {e}")))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.token.lock().unwrap() = None; // token 过期，重登
                continue;
            }
            return Ok(resp);
        }
        Err(StoreError::Protocol("auth retry exhausted".into()))
    }
}

impl BlobStore for OpenListStore {
    fn id(&self) -> &str {
        &self.config.base_url
    }

    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError> {
        let full = self.full_path(remote_key);
        let url = format!("{}/api/fs/put", self.base());
        let token = self.ensure_token()?;
        let encoded_path = encode_uri_component(&full);
        let resp = self
            .client
            .put(&url)
            .header("Authorization", &token)
            .header("File-Path", encoded_path)
            .body(data.to_vec())
            .send()
            .map_err(|e| StoreError::Protocol(format!("put: {e}")))?;
        let parsed: OpenListResp<serde_json::Value> =
            resp.json().map_err(|e| StoreError::Protocol(format!("put body: {e}")))?;
        if parsed.code != 200 {
            return Err(StoreError::Protocol(format!(
                "put: code {}: {}",
                parsed.code, parsed.message
            )));
        }
        Ok(())
    }

    fn get(&self, remote_key: &str) -> Option<Vec<u8>> {
        let full = self.full_path(remote_key);
        // /d/ 端点取不带前导斜杠的路径（避免 URL 归一化吞掉 //）
        let url = format!(
            "{}/d/{}",
            self.base(),
            encode_uri_component(full.trim_start_matches('/'))
        );
        let resp = self.authed(reqwest::Method::GET, url, None).ok()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return None;
        }
        if !resp.status().is_success() {
            return None;
        }
        resp.bytes().ok().map(|b| b.to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        // 递归走目录树（对象键按目录打散，深度 ≤4）；返回键含 prefix 子路径，
        // 以 base_path 为根（与其他 BlobStore 语义一致）。
        let root = self.config.base_path.trim_end_matches('/');
        let mut queue: Vec<String> = vec![self.full_path(prefix)];
        while let Some(dir) = queue.pop() {
            let url = format!("{}/api/fs/list", self.base());
            let resp = self.authed(
                reqwest::Method::POST,
                url,
                Some(serde_json::json!({ "path": dir, "page": 1, "per_page": 0 })),
            )?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            let parsed: OpenListResp<ListData> = resp
                .json()
                .map_err(|e| StoreError::Protocol(format!("list body: {e}")))?;
            if parsed.code != 200 {
                continue;
            }
            for entry in parsed.data.map(|d| d.content).unwrap_or_default() {
                let child_path = format!("{}/{}", dir.trim_end_matches('/'), entry.name);
                if entry.is_dir {
                    queue.push(child_path);
                } else if let Some(rel) = child_path.strip_prefix(root) {
                    out.push(rel.trim_start_matches('/').to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn delete(&self, remote_key: &str) -> Result<(), StoreError> {
        let full = self.full_path(remote_key);
        let (dir, name) = full
            .rsplit_once('/')
            .map(|(d, n)| (d.to_string(), n.to_string()))
            .ok_or_else(|| StoreError::BadKey(full.clone()))?;
        let url = format!("{}/api/fs/remove", self.base());
        let resp = self.authed(
            reqwest::Method::POST,
            url,
            Some(serde_json::json!({ "dir": dir, "names": [name] })),
        )?;
        let parsed: OpenListResp<serde_json::Value> =
            resp.json().map_err(|e| StoreError::Protocol(format!("remove body: {e}")))?;
        if parsed.code != 200 {
            return Err(StoreError::Protocol(format!(
                "remove: code {}: {}",
                parsed.code, parsed.message
            )));
        }
        Ok(())
    }
}

/// RFC 3986 组件编码（OpenList 的 File-Path 头要求）。
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests;
