//! S3 兼容存储适配器（简报 §9.3 顺序第一位的适配器）。
//!
//! 手写 AWS Signature V4（服务 `s3`），经 `reqwest` blocking + rustls 直连，
//! 不引入 AWS SDK。支持 path-style（MinIO/R2/私有部署）与 virtual-host style。
//!
//! 验证策略（简报交付纪律 §14）：
//! 1. `sigv4_*` 纯函数用 AWS 官方文档向量锁定（见 tests）；
//! 2. put/get/list/delete 用本地 mock HTTP server 做集成测试；
//! 3. 真实端点验收用 `src-tauri/src/bin/s3-verify.rs`（用户提供凭据后运行）。

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use index::store::{BlobStore, StoreError};
use sha2::{Digest, Sha256};

/// S3 连接配置（来自 Storage 配置；敏感字段在 index.db 中加密存储）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct S3Config {
    /// 例如 `https://s3.amazonaws.com` 或 `http://127.0.0.1:9000`。
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// 区域（`us-east-1`；多数 S3 兼容服务接受 `us-east-1`/`auto`）。
    pub region: String,
    /// MinIO/R2/私有部署通常 true；AWS 官方 false。
    pub path_style: bool,
}

pub struct S3Store {
    config: S3Config,
    client: reqwest::blocking::Client,
}

impl S3Store {
    pub fn new(config: S3Config) -> Result<Self, StoreError> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| StoreError::Protocol(format!("http client: {e}")))?;
        Ok(Self { config, client })
    }

    fn object_url(&self, key: &str) -> Result<String, StoreError> {
        let enc_key = encode_path(key)?;
        let base = self.config.endpoint.trim_end_matches('/');
        if self.config.path_style {
            Ok(format!("{base}/{}/{}", self.config.bucket, enc_key))
        } else {
            Ok(format!("https://{}.{}/{}", self.config.bucket, host_of(base)?, enc_key))
        }
    }

    fn list_url(&self, prefix: &str, token: Option<&str>) -> Result<String, StoreError> {
        let base = self.config.endpoint.trim_end_matches('/');
        let host_path = if self.config.path_style {
            format!("{base}/{}", self.config.bucket)
        } else {
            format!("https://{}.{}/", self.config.bucket, host_of(base)?)
        };
        let mut query = vec![("list-type".to_string(), "2".to_string())];
        if !prefix.is_empty() {
            query.push(("prefix".to_string(), prefix.to_string()));
        }
        if let Some(t) = token {
            query.push(("continuation-token".to_string(), t.to_string()));
        }
        let qs = canonical_query(&query)?;
        Ok(format!("{host_path}?{qs}"))
    }

    fn sign_request(
        &self,
        method: &str,
        url: &str,
        query: Option<&str>,
        body: &[u8],
    ) -> Result<(String, String, String), StoreError> {
        // 返回 (authorization, amz_date, payload_hash)
        let (amz_date, date) = amz_dates(SystemTime::now())?;
        let payload_hash = hex_str(&Sha256::digest(body));
        let host = url_host(url)?;
        let path = url_path(url)?;

        let mut header_pairs: Vec<(String, String)> = vec![
            ("host".into(), host),
            ("x-amz-content-sha256".into(), payload_hash.clone()),
            ("x-amz-date".into(), amz_date.clone()),
        ];
        header_pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_headers = header_pairs
            .iter()
            .map(|(k, v)| format!("{k}:{v}\n"))
            .collect::<String>();
        let signed_headers = header_pairs
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_query = query.unwrap_or("");
        let canonical_request = format!(
            "{method}\n{path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let signature = sign_canonical(
            &self.config.secret_key,
            &date,
            &self.config.region,
            &amz_date,
            &canonical_request,
        );
        let scope = format!("{}/{}/s3/aws4_request", date, self.config.region);
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.access_key, scope, signed_headers, signature
        );
        Ok((authorization, amz_date, payload_hash))
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        query: Option<&str>,
        body: Vec<u8>,
    ) -> Result<reqwest::blocking::Response, StoreError> {
        let (authorization, amz_date, payload_hash) = self.sign_request(method.as_str(), url, query, &body)?;
        let mut req = self
            .client
            .request(method, url)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", amz_date)
            .header("Authorization", authorization);
        if !body.is_empty() {
            req = req.body(body);
        }
        let resp = req.send().map_err(|e| StoreError::Protocol(format!("request: {e}")))?;
        Ok(resp)
    }

    fn expect_success(resp: reqwest::blocking::Response, what: &str) -> Result<reqwest::blocking::Response, StoreError> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp)
        } else {
            let body = resp.text().unwrap_or_default();
            Err(StoreError::Protocol(format!("{what}: HTTP {status}: {}", truncate(&body, 200))))
        }
    }
}

impl BlobStore for S3Store {
    fn id(&self) -> &str {
        &self.config.bucket
    }

    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError> {
        let url = self.object_url(remote_key)?;
        let resp = self.request(reqwest::Method::PUT, &url, None, data.to_vec())?;
        Self::expect_success(resp, "put")?;
        Ok(())
    }

    fn get(&self, remote_key: &str) -> Option<Vec<u8>> {
        let url = self.object_url(remote_key).ok()?;
        let resp = self.request(reqwest::Method::GET, &url, None, Vec::new()).ok()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return None;
        }
        let resp = Self::expect_success(resp, "get").ok()?;
        resp.bytes().ok().map(|b| b.to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let url = self.list_url(prefix, token.as_deref())?;
            let qs = url.split('?').nth(1).map(|s| s.to_string());
            let resp = self.request(reqwest::Method::GET, &url, qs.as_deref(), Vec::new())?;
            let resp = Self::expect_success(resp, "list")?;
            let xml = resp.text().map_err(|e| StoreError::Protocol(format!("list body: {e}")))?;
            out.extend(extract_keys(&xml));
            token = extract_next_token(&xml);
            if token.is_none() {
                break;
            }
        }
        Ok(out)
    }

    fn delete(&self, remote_key: &str) -> Result<(), StoreError> {
        let url = self.object_url(remote_key)?;
        let resp = self.request(reqwest::Method::DELETE, &url, None, Vec::new())?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(()); // 幂等
        }
        Self::expect_success(resp, "delete")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SigV4 纯函数
// ---------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

/// 由 canonical request 生成最终签名（AWS 官方向量锁定此函数）。
pub fn sign_canonical(
    secret_key: &str,
    date: &str,
    region: &str,
    amz_date: &str,
    canonical_request: &str,
) -> String {
    let scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex_str(&Sha256::digest(canonical_request.as_bytes()))
    );
    let signing_key = derive_signing_key(secret_key, date, region, "s3");
    let mut mac = HmacSha256::new_from_slice(&signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    hex_str(&mac.finalize().into_bytes())
}

/// AWS SigV4 签名密钥派生：HMAC 链 `AWS4<secret>` → date → region → service → "aws4_request"。
pub fn derive_signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(format!("AWS4{secret}").as_bytes()).unwrap();
    mac.update(date.as_bytes());
    let k_date = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&k_date).unwrap();
    mac.update(region.as_bytes());
    let k_region = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&k_region).unwrap();
    mac.update(service.as_bytes());
    let k_service = mac.finalize().into_bytes();
    let mut mac = HmacSha256::new_from_slice(&k_service).unwrap();
    mac.update(b"aws4_request");
    mac.finalize().into_bytes().to_vec()
}

/// URI 路径编码：除 unreserved（`A-Za-z0-9-._~`）与 `/` 外全部百分号编码。
pub fn encode_path(path: &str) -> Result<String, StoreError> {
    let mut out = String::with_capacity(path.len() * 2);
    for b in path.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Ok(out)
}

/// URI 组件编码（query 用，`/` 也编码）。
fn encode_component(s: &str) -> Result<String, StoreError> {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Ok(out)
}

/// 规范化 query：按 (key, value) 字典序排序，`k=v` 用编码值连接。
pub fn canonical_query(pairs: &[(String, String)]) -> Result<String, StoreError> {
    let mut encoded: Vec<String> = pairs
        .iter()
        .map(|(k, v)| Ok(format!("{}={}", encode_component(k)?, encode_component(v)?)))
        .collect::<Result<_, StoreError>>()?;
    encoded.sort();
    Ok(encoded.join("&"))
}

/// 从 SystemTime 生成 (amz_date=YYYYMMDDTHHMMSSZ, date=YYYYMMDD)，UTC。
pub fn amz_dates(t: SystemTime) -> Result<(String, String), StoreError> {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StoreError::Protocol(format!("clock: {e}")))?
        .as_secs();
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let date = format!("{y:04}{m:02}{d:02}");
    let amz = format!(
        "{date}T{:02}{:02}{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    );
    Ok((amz, date))
}

/// Howard Hinnant days-from-civil 反变换（1970-01-01 = day 0）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn hex_str(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn url_host(url: &str) -> Result<String, StoreError> {
    let rest = url
        .split_once("://")
        .ok_or_else(|| StoreError::BadKey(format!("bad url: {url}")))?
        .1;
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() {
        return Err(StoreError::BadKey(format!("no host: {url}")));
    }
    Ok(host.to_lowercase())
}

fn url_path(url: &str) -> Result<String, StoreError> {
    let rest = url
        .split_once("://")
        .ok_or_else(|| StoreError::BadKey(format!("bad url: {url}")))?
        .1;
    let after_host = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path_only = after_host.split('?').next().unwrap_or("");
    Ok(format!("/{path_only}"))
}

fn host_of(base: &str) -> Result<String, StoreError> {
    url_host(base)
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// 极简 ListObjectsV2 XML Key 提取（我们的键只含 [a-z0-9._-/]，仅 &amp; 转义需处理）。
fn extract_keys(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Key>") {
        let after = &rest[start + 5..];
        let Some(end) = after.find("</Key>") else { break };
        let raw = &after[..end];
        out.push(
            raw.replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">"),
        );
        rest = &after[end + 6..];
    }
    out
}

fn extract_next_token(xml: &str) -> Option<String> {
    let start = xml.find("<NextContinuationToken>")? + "<NextContinuationToken>".len();
    let rest = &xml[start..];
    let end = rest.find("</NextContinuationToken>")?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests;
