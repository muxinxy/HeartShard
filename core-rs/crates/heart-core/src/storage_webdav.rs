//! WebDAV 存储适配器（M3，简报 §9.3 第二位）。
//!
//! Rust 侧直接整块 PUT（reqwest 无内存上限）；简报 §9.2 的"分块 Content-Range
//! 续传"是鸿蒙 ArkTS 层的内存约束（ArrayBuffer），属 M4 平台层实现，不在此层。
//!
//! 兼容目标：nginx dav / Apache / Nextcloud / Alist / OpenList。
//! - `put`：PUT，409 时先 MKCOL 父目录链再重试
//! - `list`：PROPFIND Depth:1，解析 `<D:href>`（仅文件）
//! - Basic 认证

use index::store::{BlobStore, StoreError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebDavConfig {
    /// 例如 `https://dav.example.com/dav`（可含已有路径）。
    pub base_url: String,
    pub username: String,
    pub password: String,
}

pub struct WebDavStore {
    config: WebDavConfig,
    client: reqwest::blocking::Client,
}

impl WebDavStore {
    pub fn new(config: WebDavConfig) -> Result<Self, StoreError> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| StoreError::Protocol(format!("http client: {e}")))?;
        Ok(Self { config, client })
    }

    fn url_for(&self, remote_key: &str) -> Result<String, StoreError> {
        let base = self.config.base_url.trim_end_matches('/');
        Ok(format!("{base}/{}", encode_segments(remote_key)?))
    }

    fn mkcol_chain(&self, dir_path: &str) -> Result<(), StoreError> {
        let base = self.config.base_url.trim_end_matches('/');
        let mut acc = base.to_string();
        for seg in dir_path.split('/').filter(|s| !s.is_empty()) {
            acc = format!("{acc}/{}", encode_segment(seg)?);
            let resp = self
                .client
                .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &acc)
                .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
                .send()
                .map_err(|e| StoreError::Protocol(format!("mkcol: {e}")))?;
            // 405 = 已存在（允许），201 = 创建成功
            if !(resp.status().as_u16() == 405 || resp.status().is_success()) {
                return Err(StoreError::Protocol(format!(
                    "mkcol {}: HTTP {}",
                    acc,
                    resp.status()
                )));
            }
        }
        Ok(())
    }
}

fn encode_segment(seg: &str) -> Result<String, StoreError> {
    let mut out = String::new();
    for b in seg.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Ok(out)
}

fn encode_segments(key: &str) -> Result<String, StoreError> {
    Ok(key
        .split('/')
        .map(encode_segment)
        .collect::<Result<Vec<_>, _>>()?
        .join("/"))
}

impl BlobStore for WebDavStore {
    fn id(&self) -> &str {
        &self.config.base_url
    }

    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError> {
        let url = self.url_for(remote_key)?;
        let resp = self
            .client
            .put(&url)
            .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
            .body(data.to_vec())
            .send()
            .map_err(|e| StoreError::Protocol(format!("put: {e}")))?;
        let status = resp.status().as_u16();
        if status == 409 {
            // 父集合不存在：MKCOL 链后重试一次
            let parent = remote_key
                .rsplit_once('/')
                .map(|(p, _)| p)
                .unwrap_or_default();
            if !parent.is_empty() {
                self.mkcol_chain(parent)?;
                let resp = self
                    .client
                    .put(&url)
                    .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
                    .body(data.to_vec())
                    .send()
                    .map_err(|e| StoreError::Protocol(format!("put retry: {e}")))?;
                return if resp.status().is_success() {
                    Ok(())
                } else {
                    Err(StoreError::Protocol(format!("put retry: HTTP {}", resp.status())))
                };
            }
        }
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(StoreError::Protocol(format!("put: HTTP {status}")))
        }
    }

    fn get(&self, remote_key: &str) -> Option<Vec<u8>> {
        let url = self.url_for(remote_key).ok()?;
        let resp = self
            .client
            .get(&url)
            .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
            .send()
            .ok()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return None;
        }
        if !resp.status().is_success() {
            return None;
        }
        resp.bytes().ok().map(|b| b.to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let url = self.url_for(prefix)?;
        let resp = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
            .header("Depth", "1")
            .body(
                r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#,
            )
            .send()
            .map_err(|e| StoreError::Protocol(format!("propfind: {e}")))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !resp.status().is_success() && resp.status().as_u16() != 207 {
            return Err(StoreError::Protocol(format!("propfind: HTTP {}", resp.status())));
        }
        let xml = resp.text().map_err(|e| StoreError::Protocol(format!("propfind body: {e}")))?;

        // 解析 <D:href>（大小写两种命名空间），扣除 base 前缀得到相对 key
        let base_path = url_path(&self.config.base_url);
        let mut keys = Vec::new();
        for href in extract_hrefs(&xml) {
            let path = percent_decode(&href_path(&href));
            let Some(rel) = path.strip_prefix(&base_path) else { continue };
            let rel = rel.trim_start_matches('/');
            if rel.is_empty() || rel == prefix.trim_end_matches('/') {
                continue; // 集合自身
            }
            if rel.ends_with('/') {
                continue; // 子目录不递归（Depth 1）
            }
            keys.push(rel.to_string());
        }
        keys.sort();
        Ok(keys)
    }

    fn delete(&self, remote_key: &str) -> Result<(), StoreError> {
        let url = self.url_for(remote_key)?;
        let resp = self
            .client
            .delete(&url)
            .basic_auth(self.config.username.clone(), Some(self.config.password.clone()))
            .send()
            .map_err(|e| StoreError::Protocol(format!("delete: {e}")))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND || resp.status().is_success() {
            Ok(())
        } else {
            Err(StoreError::Protocol(format!("delete: HTTP {}", resp.status())))
        }
    }
}

fn url_path(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path_only = path.split('?').next().unwrap_or("");
    format!("/{path_only}")
}

fn href_path(href: &str) -> String {
    // href 可能是完整 URL 或绝对路径
    let p = match href.split_once("://") {
        Some((_, r)) => r.split_once('/').map(|(_, p)| format!("/{p}")).unwrap_or("/".into()),
        None => href.to_string(),
    };
    p.split('?').next().unwrap_or("/").to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        }
    String::from_utf8_lossy(&out).to_string()
}

fn extract_hrefs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in ["<D:href>", "<d:href>", "<href>"] {
        let mut rest = xml;
        while let Some(start) = rest.find(tag) {
            let after = &rest[start + tag.len()..];
            let Some(end) = after.find("</") else { break };
            out.push(after[..end].to_string());
            rest = &after[end..];
        }
    }
    out
}

#[cfg(test)]
mod tests;
