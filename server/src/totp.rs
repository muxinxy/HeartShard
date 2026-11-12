//! TOTP 2fa（RFC 6238，30s 步长 / 6 位 / SHA1）：生成密钥、校验码、otpauth URI。
//! base32 编解码用于密钥的展示与存储（用户在验证器 App 里手动录入）。

use base32::Alphabet;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

/// 生成新密钥（base32，20 字节熵）。
pub fn generate_secret() -> String {
    let mut buf = [0u8; 20];
    getrandom::getrandom(&mut buf).expect("OS RNG");
    base32::encode(Alphabet::Rfc4648Lower { padding: false }, &buf)
}

/// 校验 6 位验证码（±1 窗口容差）。
pub fn verify(secret_b32: &str, code: &str) -> bool {
    let secret = match base32::decode(Alphabet::Rfc4648Lower { padding: false }, &secret_b32.trim().to_lowercase()) {
        Some(s) => s,
        None => return false,
    };
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 30)
        .unwrap_or(0);
    for drift in [-1i64, 0, 1] {
        let counter = (now as i64 + drift) as u64;
        if totp_at(&secret, counter) == code {
            return true;
        }
    }
    false
}

/// otpauth:// URI（验证器 App 扫码/手动录入用）。
pub fn otpauth_uri(secret_b32: &str, account: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={secret_b32}&issuer={issuer}&algorithm=SHA1&digits=6&period=30"
    )
}

fn totp_at(secret: &[u8], counter: u64) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("hmac key");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0F) as usize;
    let bin = ((digest[offset] & 0x7F) as u32) << 24
        | (digest[offset + 1] as u32) << 16
        | (digest[offset + 2] as u32) << 8
        | digest[offset + 3] as u32;
    format!("{:06}", bin % 1_000_000)
}

#[derive(Serialize, Deserialize)]
pub struct TotpSetup {
    pub secret: String,
    pub uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 附录 B 的 SHA1 测试向量（密钥 "12345678901234567890"）。
    #[test]
    fn rfc6238_vectors() {
        let secret = base32::decode(Alphabet::Rfc4648Lower { padding: false }, &base32::encode(Alphabet::Rfc4648Lower { padding: false }, b"12345678901234567890")).unwrap();
        // T=59 → 94287082（截断后 287082）
        assert_eq!(totp_at(&secret, 59 / 30), "287082");
        // T=1111111109 → 081804（截断后 081804）
        assert_eq!(totp_at(&secret, 1111111109 / 30), "081804");
    }

    #[test]
    fn verify_roundtrip() {
        let secret = generate_secret();
        let code = totp_at(
            &base32::decode(Alphabet::Rfc4648Lower { padding: false }, &secret).unwrap(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 30,
        );
        assert!(verify(&secret, &code));
        assert!(!verify(&secret, "000000z"));
    }
}
