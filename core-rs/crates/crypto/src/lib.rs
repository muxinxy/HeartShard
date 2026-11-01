//! HeartShard 分片加密与密钥派生。
//!
//! 两层格式（详见 `docs/mixfile-study.md`）：
//! - **bare 层**（MixFile 对齐基线，仅用于测试向量与算法锁定）：
//!   AES-256-GCM，12B 随机 IV 前置，96-bit tag，无 AAD，输出 `iv‖ct‖tag`。
//! - **chunk 层**（HeartShard 生产格式，裁决 Q2）：
//!   每 blob 独立密钥 `blob_key = HKDF(MK, salt=master_salt, info="heartshard/blob/v1"‖blob_hash)`，
//!   每 chunk 随机 12B IV，
//!   AAD = `blob_hash(32B) ‖ chunk_index(u32 BE) ‖ total_chunks(u32 BE)`，
//!   输出同样是 `iv‖ct‖tag`。
//!
//! 安全约束：口令不落盘，MK 只在会话内存（返回类型用 `Zeroizing` 包裹）。

use aes::{Aes128, Aes192, Aes256};
use aes_gcm::{
    aead::{consts::U12, Aead, KeyInit, Payload},
    AesGcm,
};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

pub mod base62;

/// GCM IV/Nonce 长度（字节）。
pub const IV_LEN: usize = 12;
/// GCM 认证标签长度（字节）。MixFile 用 96-bit，非 JCE/常见默认的 128-bit。
pub const TAG_LEN: usize = 12;
/// blob 哈希原始字节数（SHA-256）。
pub const BLOB_HASH_LEN: usize = 32;

type Gcm128 = AesGcm<Aes128, U12, U12>;
type Gcm192 = AesGcm<Aes192, U12, U12>;
type Gcm256 = AesGcm<Aes256, U12, U12>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CryptoError {
    #[error("AES key must be 16, 24 or 32 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("IV must be {IV_LEN} bytes, got {0}")]
    InvalidIvLength(usize),
    #[error("encrypted data too short (need > {min} bytes, got {got})")]
    DataTooShort { got: usize, min: usize },
    #[error("AES-GCM operation failed (wrong key, corrupted data, or AAD mismatch)")]
    Aead,
    #[error("invalid base62 string")]
    Base62,
    #[error("argon2 KDF failed: {0}")]
    Kdf(String),
}

impl From<aes_gcm::Error> for CryptoError {
    fn from(_: aes_gcm::Error) -> Self {
        // aes_gcm::Error 不携带细节，统一视为认证失败/输入损坏
        CryptoError::Aead
    }
}

/// 按 key 长度分派 AES-GCM（tag 固定 12 字节）。
enum Cipher {
    A128(Box<Gcm128>),
    A192(Box<Gcm192>),
    A256(Box<Gcm256>),
}

impl Cipher {
    fn new(key: &[u8]) -> Result<Self, CryptoError> {
        match key.len() {
            16 => Ok(Cipher::A128(Box::new(
                Gcm128::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength(key.len()))?,
            ))),
            24 => Ok(Cipher::A192(Box::new(
                Gcm192::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength(key.len()))?,
            ))),
            32 => Ok(Cipher::A256(Box::new(
                Gcm256::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength(key.len()))?,
            ))),
            n => Err(CryptoError::InvalidKeyLength(n)),
        }
    }

    /// 返回 `ct‖tag(12B)`。
    fn seal(&self, iv: &[u8; IV_LEN], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let payload = Payload { msg: plaintext, aad };
        let nonce = aes_gcm::Nonce::<U12>::from_slice(iv);
        let ct = match self {
            Cipher::A128(c) => c.encrypt(nonce, payload)?,
            Cipher::A192(c) => c.encrypt(nonce, payload)?,
            Cipher::A256(c) => c.encrypt(nonce, payload)?,
        };
        debug_assert_eq!(ct.len(), plaintext.len() + TAG_LEN);
        Ok(ct)
    }

    fn open(&self, iv: &[u8; IV_LEN], ct_with_tag: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let payload = Payload { msg: ct_with_tag, aad };
        let nonce = aes_gcm::Nonce::<U12>::from_slice(iv);
        let pt = match self {
            Cipher::A128(c) => c.decrypt(nonce, payload)?,
            Cipher::A192(c) => c.decrypt(nonce, payload)?,
            Cipher::A256(c) => c.decrypt(nonce, payload)?,
        };
        Ok(pt)
    }
}

// ---------------------------------------------------------------------------
// MixFile 对齐 bare 层（AES.kt 的逐字节等价物；无 AAD）
// ---------------------------------------------------------------------------

/// MixFile `encryptAES`：输出 `iv(12B) ‖ ciphertext ‖ tag(12B)`，无 AAD。
/// key 支持 16/24/32 字节（MixFile 主数据 32B，分享码层 16B）。
pub fn encrypt_bare(key: &[u8], iv: &[u8; IV_LEN], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Cipher::new(key)?;
    let ct = cipher.seal(iv, plaintext, b"")?;
    let mut out = Vec::with_capacity(IV_LEN + ct.len());
    out.extend_from_slice(iv);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// MixFile `decryptAES`：输入头 12 字节为 IV，其余为 `ct‖tag`，无 AAD。
pub fn decrypt_bare(key: &[u8], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() <= IV_LEN {
        return Err(CryptoError::DataTooShort { got: data.len(), min: IV_LEN });
    }
    let (iv, rest) = data.split_at(IV_LEN);
    let mut iv_arr = [0u8; IV_LEN];
    iv_arr.copy_from_slice(iv);
    let cipher = Cipher::new(key)?;
    cipher.open(&iv_arr, rest, b"")
}

/// SHA-256（供上层与测试使用；单实现，避免各处引入不同 hash 依赖）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// MixFile `hashMixSHA256`：`base62(SHA-256(data))`。
pub fn hash_mix_sha256(data: &[u8]) -> String {
    base62::encode(&sha256(data))
}

/// 生成随机 12B IV（每 chunk 独立；裁决 Q2）。
pub fn generate_iv() -> Result<[u8; IV_LEN], CryptoError> {
    let mut iv = [0u8; IV_LEN];
    getrandom::getrandom(&mut iv).map_err(|e| CryptoError::Kdf(format!("getrandom: {e}")))?;
    Ok(iv)
}

// ---------------------------------------------------------------------------
// KDF：口令 → Argon2id → MK → HKDF → 子密钥
// ---------------------------------------------------------------------------

/// Argon2id 参数。默认 m=64 MiB, t=3, p=4（简报值，OWASP 区间内）；
/// 低配移动端（低端 Android/鸿蒙）可用 [`KdfParams::low`]（32 MiB）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// 内存开销（KiB）。
    pub m_cost_kib: u32,
    /// 迭代轮数。
    pub t_cost: u32,
    /// 并行度。
    pub parallelism: u32,
}

impl KdfParams {
    pub const fn standard() -> Self {
        Self { m_cost_kib: 64 * 1024, t_cost: 3, parallelism: 4 }
    }

    pub const fn low() -> Self {
        Self { m_cost_kib: 32 * 1024, t_cost: 3, parallelism: 4 }
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::standard()
    }
}

/// 由口令派生主密钥 MK（32B）。salt 至少 8 字节，推荐 16 字节随机值，随密钥封装参数落盘。
pub fn derive_master_key(
    password: &[u8],
    salt: &[u8],
    params: &KdfParams,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.m_cost_kib, params.t_cost, params.parallelism, Some(32))
            .map_err(|e| CryptoError::Kdf(format!("params: {e}")))?,
    );
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password, salt, out.as_mut())
        .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    Ok(out)
}

const HKDF_LABEL_MANIFEST: &[u8] = b"heartshard/manifest/v1";
const HKDF_LABEL_CONFIG: &[u8] = b"heartshard/config/v1";
const HKDF_LABEL_BLOB: &[u8] = b"heartshard/blob/v1";
const HKDF_LABEL_INDEX: &[u8] = b"heartshard/index/v1";

/// 从 MK 派生的会话子密钥集合。持会话生命周期，Drop 后密文零化
/// （`Hkdf` 内部 prk 不做零化——MK 本就在会话内存中，见 docs/mixfile-study.md §5 Q2 注）。
/// `Clone` 供命令层跨作用域传递；副本 Drop 时同样零化。
#[derive(Clone)]
pub struct DerivedKeys {
    master_key: Zeroizing<[u8; 32]>,
    master_salt: Vec<u8>,
    pub manifest_key: Zeroizing<[u8; 32]>,
    pub config_key: Zeroizing<[u8; 32]>,
    /// SQLCipher index.db 密钥（HKDF，§7.1）。
    pub index_key: Zeroizing<[u8; 32]>,
}

impl DerivedKeys {
    /// `master_salt` 为安装时生成的随机盐（≥16B，随密钥封装参数落盘）。
    pub fn new(master_key: Zeroizing<[u8; 32]>, master_salt: &[u8]) -> Result<Self, CryptoError> {
        let keys = Self {
            manifest_key: Zeroizing::new(expand(&master_key, master_salt, HKDF_LABEL_MANIFEST)?),
            config_key: Zeroizing::new(expand(&master_key, master_salt, HKDF_LABEL_CONFIG)?),
            index_key: Zeroizing::new(expand(&master_key, master_salt, HKDF_LABEL_INDEX)?),
            master_key,
            master_salt: master_salt.to_vec(),
        };
        Ok(keys)
    }

    /// 每 blob 独立加密密钥（裁决 Q2）：
    /// `HKDF-Expand(prk, info = "heartshard/blob/v1" ‖ blob_hash(32B))`。
    /// 不同 blob 的 key 相互独立，chunk 级随机 IV 无重用风险。
    pub fn blob_key(&self, blob_hash: &[u8; BLOB_HASH_LEN]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let mut info = Vec::with_capacity(HKDF_LABEL_BLOB.len() + BLOB_HASH_LEN);
        info.extend_from_slice(HKDF_LABEL_BLOB);
        info.extend_from_slice(blob_hash);
        Ok(Zeroizing::new(expand(&self.master_key, &self.master_salt, &info)?))
    }
}

fn expand(mk: &[u8; 32], salt: &[u8], info: &[u8]) -> Result<[u8; 32], CryptoError> {
    let hk = Hkdf::<Sha256>::new(Some(salt), mk);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm).map_err(|e| CryptoError::Kdf(format!("hkdf expand: {e}")))?;
    Ok(okm)
}

// ---------------------------------------------------------------------------
// HeartShard chunk 层（生产格式：per-blob key + 随机 IV + AAD）
// ---------------------------------------------------------------------------

/// chunk AAD：`blob_hash(32B) ‖ chunk_index(u32 BE) ‖ total_chunks(u32 BE)`。
/// 绑定 blob 身份与分片位置，防重排/截断/跨 blob 拼接。
fn chunk_aad(blob_hash: &[u8; BLOB_HASH_LEN], chunk_index: u32, total_chunks: u32) -> [u8; 40] {
    let mut aad = [0u8; 40];
    aad[..32].copy_from_slice(blob_hash);
    aad[32..36].copy_from_slice(&chunk_index.to_be_bytes());
    aad[36..40].copy_from_slice(&total_chunks.to_be_bytes());
    aad
}

/// 加密一个 chunk（HeartShard 生产格式）。
/// 输出 `iv(12B) ‖ ciphertext ‖ tag(12B)`；IV 必须每 chunk 随机（[`generate_iv`]）。
pub fn encrypt_chunk(
    blob_key: &[u8; 32],
    iv: &[u8; IV_LEN],
    blob_hash: &[u8; BLOB_HASH_LEN],
    chunk_index: u32,
    total_chunks: u32,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Cipher::new(blob_key)?;
    let aad = chunk_aad(blob_hash, chunk_index, total_chunks);
    let ct = cipher.seal(iv, plaintext, &aad)?;
    let mut out = Vec::with_capacity(IV_LEN + ct.len());
    out.extend_from_slice(iv);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 解密一个 chunk（验证 AAD 与 tag；任何不匹配返回 [`CryptoError::Aead`]）。
pub fn decrypt_chunk(
    blob_key: &[u8; 32],
    data: &[u8],
    blob_hash: &[u8; BLOB_HASH_LEN],
    chunk_index: u32,
    total_chunks: u32,
) -> Result<Vec<u8>, CryptoError> {
    if data.len() <= IV_LEN {
        return Err(CryptoError::DataTooShort { got: data.len(), min: IV_LEN });
    }
    let (iv, rest) = data.split_at(IV_LEN);
    let mut iv_arr = [0u8; IV_LEN];
    iv_arr.copy_from_slice(iv);
    let cipher = Cipher::new(blob_key)?;
    let aad = chunk_aad(blob_hash, chunk_index, total_chunks);
    cipher.open(&iv_arr, rest, &aad)
}
