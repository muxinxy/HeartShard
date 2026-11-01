//! FTP / FTPS 存储适配器（简报 §9.3 最后一位；移动端兼容性最差，桌面优先）。
//!
//! 基于 `suppaftp` 12（FTPS 用 `rustls-ring` 特性，纯 Rust TLS）。
//! 连接策略：每次操作短连接（M5 简化；连接池属后续优化）。
//!
//! ⚠️ 交付纪律（简报 §14）：上线前必须用真实 FTP/FTPS 服务跑通
//! put/get/list/delete（无本地 FTP 服务器，单测只覆盖纯逻辑与错误映射）。

use std::io::Cursor;
use std::sync::Arc;

use index::store::{BlobStore, StoreError};
use suppaftp::{RustlsConnector, RustlsFtpStream};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// true = FTPS（AUTH TLS，证书校验走 webpki roots）；false = 明文 FTP（仅限可信内网）。
    pub secure: bool,
    /// 登录后的基础目录（如 "/heartshard"），对象键附加在其后。
    pub base_path: String,
}

pub struct FtpStore {
    config: FtpConfig,
}

fn ftp_err(e: suppaftp::FtpError) -> StoreError {
    StoreError::Protocol(format!("ftp: {e}"))
}

impl FtpStore {
    pub fn new(config: FtpConfig) -> Result<Self, StoreError> {
        Ok(Self { config })
    }

    fn connect(&self) -> Result<RustlsFtpStream, StoreError> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        if !self.config.secure {
            return Err(StoreError::Protocol(
                "plain FTP is disabled by default (credentials in cleartext); use FTPS".into(),
            ));
        }
        let plain = RustlsFtpStream::connect(addr).map_err(ftp_err)?;
        let mut roots = suppaftp::rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = suppaftp::rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let mut stream =
            plain.into_secure(RustlsConnector::from(Arc::new(config)), &self.config.host).map_err(ftp_err)?;
        stream.login(&self.config.username, &self.config.password).map_err(ftp_err)?;
        stream.transfer_type(suppaftp::types::FileType::Binary).map_err(ftp_err)?;
        let base = self.config.base_path.trim_matches('/');
        if !base.is_empty() {
            for seg in base.split('/') {
                let _ = stream.mkdir(seg); // 550 已存在，忽略
                stream.cwd(seg).map_err(ftp_err)?;
            }
        }
        Ok(stream)
    }

    /// 进入对象键的父目录链（相对 base_path）。
    fn cd_into(&self, stream: &mut RustlsFtpStream, dir: &str) -> Result<(), StoreError> {
        for seg in dir.split('/').filter(|s| !s.is_empty()) {
            let _ = stream.mkdir(seg);
            stream.cwd(seg).map_err(ftp_err)?;
        }
        Ok(())
    }
}

impl BlobStore for FtpStore {
    fn id(&self) -> &str {
        &self.config.host
    }

    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError> {
        let mut stream = self.connect()?;
        if let Some((dir, _)) = remote_key.rsplit_once('/') {
            self.cd_into(&mut stream, dir)?;
        }
        let mut reader = Cursor::new(data.to_vec());
        stream.put_file(remote_key, &mut reader).map_err(ftp_err)?;
        let _ = stream.quit();
        Ok(())
    }

    fn get(&self, remote_key: &str) -> Option<Vec<u8>> {
        let mut stream = self.connect().ok()?;
        let data = match stream.retr_as_buffer(remote_key) {
            Ok(cursor) => cursor.into_inner(),
            // 服务器以 550 回应不存在；BadResponse 统一按缺失处理（见模块注释：待真实验证细化）
            Err(suppaftp::FtpError::BadResponse) => {
                let _ = stream.quit();
                return None;
            }
            Err(e) => {
                let _ = stream.quit();
                eprintln!("ftp get {}: {e}", self.config.host);
                return None;
            }
        };
        let _ = stream.quit();
        Some(data)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut stream = self.connect()?;
        let (dir, file_part) = match prefix.rsplit_once('/') {
            Some((d, f)) => (d, f),
            None => ("", prefix),
        };
        if !dir.is_empty() {
            self.cd_into(&mut stream, dir)?;
        }
        let names = stream.nlst(None).map_err(ftp_err)?;
        let mut out: Vec<String> = names
            .into_iter()
            .filter(|n| file_part.is_empty() || n.starts_with(file_part))
            .map(|n| {
                if dir.is_empty() {
                    n
                } else {
                    format!("{dir}/{n}")
                }
            })
            .collect();
        out.sort();
        let _ = stream.quit();
        Ok(out)
    }

    fn delete(&self, remote_key: &str) -> Result<(), StoreError> {
        let mut stream = self.connect()?;
        match stream.rm(remote_key) {
            Ok(()) => {}
            Err(suppaftp::FtpError::BadResponse) => {} // 550 不存在 → 幂等成功
            Err(e) => return Err(ftp_err(e)),
        }
        let _ = stream.quit();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ftp_rejected_by_default() {
        let store = FtpStore::new(FtpConfig {
            host: "127.0.0.1".into(),
            port: 2121,
            username: "u".into(),
            password: "p".into(),
            secure: false,
            base_path: "/heartshard".into(),
        })
        .unwrap();
        // 明文 FTP 默认拒绝（凭据明文传输），强制走 FTPS
        let err = store.put("blobs/aa/bb/00000_00.shard", b"x").unwrap_err();
        assert!(err.to_string().contains("plain FTP is disabled"));
    }

    #[test]
    fn ftps_connect_failure_maps_to_store_error() {
        let store = FtpStore::new(FtpConfig {
            host: "127.0.0.1".into(),
            port: 1, // 不可达端口
            username: "u".into(),
            password: "p".into(),
            secure: true,
            base_path: "/heartshard".into(),
        })
        .unwrap();
        let err = store.put("blobs/aa/bb/00000_00.shard", b"x").unwrap_err();
        assert!(err.to_string().contains("ftp: Connection error"), "{err}");
    }
}
