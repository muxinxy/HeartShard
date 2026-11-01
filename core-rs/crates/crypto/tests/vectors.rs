//! 测试向量锁定（M0 退出标准：逐字节匹配）。
//!
//! - `docs/fixtures/mixfile-vectors.json`：Java JCE 生成（与 mixfile-core 同调用路径），
//!   Python/OpenSSL 交叉验证过；此处断言 Rust 实现逐字节复现。
//! - `docs/fixtures/heartshard-vectors.json`：Python cryptography + argon2-cffi 生成的
//!   HeartShard 生产格式（AAD/blob_key/KDF）交叉向量。

use crypto::{
    base62, decrypt_bare, decrypt_chunk, derive_master_key, encrypt_bare, encrypt_chunk,
    hash_mix_sha256, sha256, CryptoError, DerivedKeys, KdfParams,
};
use serde_json::Value;

const MIXFILE_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/fixtures/mixfile-vectors.json"
);
const HEARTSHARD_JSON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/fixtures/heartshard-vectors.json"
);

fn load(path: &str) -> Value {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{path} is not valid JSON: {e}"))
}

fn mixfile_vector(id: &str) -> Value {
    let doc = load(MIXFILE_JSON);
    doc["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("vector {id} not found"))
        .clone()
}

/// 与 VectorGen.java 相同的确定性伪随机流：SHA-256(seed || counter32BE) 链式展开。
fn prng(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 32);
    let mut counter: u32 = 0;
    while out.len() < len {
        let mut block = Vec::with_capacity(seed.len() + 4);
        block.extend_from_slice(seed);
        block.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&sha256(&block));
        counter += 1;
    }
    out.truncate(len);
    out
}

// ---------------------------------------------------------------------------
// MixFile bare 层
// ---------------------------------------------------------------------------

#[test]
fn v1_basic_64b_byte_exact() {
    let v = mixfile_vector("V1");
    let key = hex::decode(v["key_hex"].as_str().unwrap()).unwrap();
    let iv_hex = v["iv_hex"].as_str().unwrap();
    let pt = hex::decode(v["plaintext_hex"].as_str().unwrap()).unwrap();
    let ct_hex = v["ciphertext_hex"].as_str().unwrap();

    let mut iv = [0u8; 12];
    iv.copy_from_slice(&hex::decode(iv_hex).unwrap());

    let ct = encrypt_bare(&key, &iv, &pt).unwrap();
    assert_eq!(hex::encode(&ct), ct_hex, "V1 ciphertext must match byte-for-byte");

    let back = decrypt_bare(&key, &ct).unwrap();
    assert_eq!(back, pt);

    // hashMixSHA256 = base62(sha256(plaintext))
    assert_eq!(hash_mix_sha256(&pt), v["plaintext_sha256_base62"].as_str().unwrap());
    assert_eq!(hex::encode(sha256(&pt)), v["plaintext_sha256_hex"].as_str().unwrap());
}

#[test]
fn v2_multi_chunk_and_index_byte_exact() {
    let v = mixfile_vector("V2");
    let key = hex::decode(v["key_hex"].as_str().unwrap()).unwrap();
    let chunk_size = v["chunk_size"].as_u64().unwrap() as usize;
    let total_len = v["plaintext_len"].as_u64().unwrap() as usize;
    let blob = prng(v["prng_seed"].as_str().unwrap().as_bytes(), total_len);
    assert_eq!(blob.len(), chunk_size + chunk_size / 2);

    for (name, offset) in [("chunk0", 0usize), ("chunk1", chunk_size)] {
        let c = &v[name];
        let chunk = &blob[offset..offset + c["pt_len"].as_u64().unwrap() as usize];
        assert_eq!(
            hex::encode(sha256(chunk)),
            c["pt_sha256_hex"].as_str().unwrap(),
            "{name} plaintext hash (PRNG reproduction)"
        );
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&hex::decode(c["iv_hex"].as_str().unwrap()).unwrap());
        let ct = encrypt_bare(&key, &iv, chunk).unwrap();
        assert_eq!(
            hex::encode(sha256(&ct)),
            c["ct_sha256_hex"].as_str().unwrap(),
            "{name} ciphertext (wire iv||ct||tag12) hash"
        );
        assert_eq!(ct.len(), c["ct_len"].as_u64().unwrap() as usize);
        assert_eq!(&hex::encode(&ct[..32]), c["ct_first32_hex"].as_str().unwrap());
        assert_eq!(
            &hex::encode(&ct[ct.len() - 32..]),
            c["ct_last32_hex"].as_str().unwrap()
        );
    }

    // 索引链：gzip 字节（Java 基准）→ AES → 密文逐字节
    let index_json = v["index_json"].as_str().unwrap();
    let gzip = hex::decode(v["index_gzip_hex"].as_str().unwrap()).unwrap();
    let mut iv_c = [0u8; 12];
    iv_c.copy_from_slice(&hex::decode(v["index_encrypted_iv_hex"].as_str().unwrap()).unwrap());
    let enc_index = encrypt_bare(&key, &iv_c, &gzip).unwrap();
    assert_eq!(
        hex::encode(&enc_index),
        v["index_encrypted_hex"].as_str().unwrap(),
        "index ciphertext must match byte-for-byte"
    );
    let dec = decrypt_bare(&key, &enc_index).unwrap();
    assert_eq!(dec, gzip);
    // gzip 层跨语言只要求可解压且内容一致
    let mut gz = flate2::read::GzDecoder::new(&gzip[..]);
    let mut json_out = String::new();
    std::io::Read::read_to_string(&mut gz, &mut json_out).unwrap();
    assert_eq!(json_out, index_json, "decompressed index JSON must match");
}

#[test]
fn v3_share_code_byte_exact() {
    let v = mixfile_vector("V3");
    let key = hex::decode(v["key_hex"].as_str().unwrap()).unwrap(); // MD5("123"), 16B
    assert_eq!(key.len(), 16);
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&hex::decode(v["iv_hex"].as_str().unwrap()).unwrap());
    let pt = v["plaintext_utf8"].as_str().unwrap();

    let ct = encrypt_bare(&key, &iv, pt.as_bytes()).unwrap();
    assert_eq!(hex::encode(&ct), v["ciphertext_hex"].as_str().unwrap());
    let code = base62::encode(&ct);
    assert_eq!(code, v["share_code"].as_str().unwrap(), "share code must match");
    assert_eq!(base62::decode(&code).unwrap(), ct);
    // 解密侧：分享码 → base62 解码 → AES 解密 → MixShareInfo JSON
    let back = decrypt_bare(&key, &base62::decode(&code).unwrap()).unwrap();
    assert_eq!(back, pt.as_bytes());
}

#[test]
fn v4_empty_plaintext() {
    let v = mixfile_vector("V4");
    let key = hex::decode(v["key_hex"].as_str().unwrap()).unwrap();
    let mut iv = [0u8; 12];
    iv.copy_from_slice(&hex::decode(v["iv_hex"].as_str().unwrap()).unwrap());
    let ct = encrypt_bare(&key, &iv, b"").unwrap();
    assert_eq!(hex::encode(&ct), v["ciphertext_hex"].as_str().unwrap());
    assert_eq!(ct.len(), 24); // iv(12) + tag(12)
    assert_eq!(decrypt_bare(&key, &ct).unwrap(), Vec::<u8>::new());
}

#[test]
fn bare_layer_rejects_tampering() {
    let key = [0u8; 32];
    let iv = [0u8; 12];
    let mut ct = encrypt_bare(&key, &iv, b"attack at dawn").unwrap();
    let last = ct.len() - 1;
    ct[last] ^= 0x01;
    assert!(matches!(decrypt_bare(&key, &ct), Err(CryptoError::Aead)));
    // 截断
    assert!(matches!(
        decrypt_bare(&key, &ct[..ct.len() - 1]),
        Err(CryptoError::Aead)
    ));
    // 过短
    assert!(matches!(
        decrypt_bare(&key, &[0u8; 12]),
        Err(CryptoError::DataTooShort { .. })
    ));
    // 非法 key 长度
    assert!(matches!(
        encrypt_bare(&[0u8; 17], &iv, b"x"),
        Err(CryptoError::InvalidKeyLength(17))
    ));
}

// ---------------------------------------------------------------------------
// HeartShard 生产格式（KDF + AAD chunk）
// ---------------------------------------------------------------------------

#[test]
fn heartshard_kdf_cross_validated() {
    let doc = load(HEARTSHARD_JSON);
    let k = &doc["kdf"];
    let password = k["password_utf8"].as_str().unwrap();
    let argon_salt = hex::decode(k["argon2_salt_hex"].as_str().unwrap()).unwrap();
    let master_salt = k["master_salt_utf8"].as_str().unwrap().as_bytes();

    // Argon2id（m=64MiB, t=3, p=4）——与 argon2-cffi 逐字节一致
    let mk = derive_master_key(password.as_bytes(), &argon_salt, &KdfParams::standard()).unwrap();
    assert_eq!(hex::encode(mk.as_ref()), k["master_key_hex"].as_str().unwrap(), "MK");

    let keys = DerivedKeys::new(mk, master_salt).unwrap();
    assert_eq!(
        hex::encode(keys.manifest_key.as_ref()),
        k["manifest_key_hex"].as_str().unwrap(),
        "manifest_key"
    );
    assert_eq!(
        hex::encode(keys.config_key.as_ref()),
        k["config_key_hex"].as_str().unwrap(),
        "config_key"
    );
    let blob_hash: [u8; 32] = hex::decode(k["blob_hash_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let blob_key = keys.blob_key(&blob_hash).unwrap();
    assert_eq!(
        hex::encode(blob_key.as_ref()),
        k["blob_key_hex"].as_str().unwrap(),
        "blob_key (HKDF info = label || blob_hash)"
    );
}

#[test]
fn heartshard_chunk_aad_byte_exact() {
    let doc = load(HEARTSHARD_JSON);
    for case in doc["chunk"].as_array().unwrap() {
        let blob_key: [u8; 32] = hex::decode(case["blob_key_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let blob_hash: [u8; 32] = hex::decode(case["blob_hash_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&hex::decode(case["iv_hex"].as_str().unwrap()).unwrap());
        let pt = case["plaintext_utf8"].as_str().unwrap();

        let ct = encrypt_chunk(
            &blob_key,
            &iv,
            &blob_hash,
            case["chunk_index"].as_u64().unwrap() as u32,
            case["total_chunks"].as_u64().unwrap() as u32,
            pt.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            hex::encode(&ct),
            case["ciphertext_hex"].as_str().unwrap(),
            "{} ciphertext must match byte-for-byte",
            case["id"].as_str().unwrap()
        );
        let back = decrypt_chunk(
            &blob_key,
            &ct,
            &blob_hash,
            case["chunk_index"].as_u64().unwrap() as u32,
            case["total_chunks"].as_u64().unwrap() as u32,
        )
        .unwrap();
        assert_eq!(back, pt.as_bytes());
    }
}

#[test]
fn heartshard_aad_binding_rejects_mismatch() {
    // 密文对 (blob_hash, chunk_index, total) 绑定：任何一项不匹配都必须失败
    let blob_key = [0x42u8; 32];
    let blob_hash = sha256(b"some blob");
    let other_hash = sha256(b"another blob");
    let iv = [0u8; 12];
    let ct = encrypt_chunk(&blob_key, &iv, &blob_hash, 1, 3, b"data").unwrap();

    assert!(matches!(
        decrypt_chunk(&blob_key, &ct, &blob_hash, 0, 3), // chunk_index 不匹配
        Err(CryptoError::Aead)
    ));
    assert!(matches!(
        decrypt_chunk(&blob_key, &ct, &blob_hash, 1, 4), // total 不匹配
        Err(CryptoError::Aead)
    ));
    assert!(matches!(
        decrypt_chunk(&blob_key, &ct, &other_hash, 1, 3), // blob_hash 不匹配
        Err(CryptoError::Aead)
    ));
    let mut tampered = ct.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x80; // 破坏 tag
    assert!(matches!(
        decrypt_chunk(&blob_key, &tampered, &blob_hash, 1, 3),
        Err(CryptoError::Aead)
    ));
}

#[test]
fn kdf_params_documented_defaults() {
    let p = KdfParams::standard();
    assert_eq!((p.m_cost_kib, p.t_cost, p.parallelism), (65536, 3, 4));
    let lo = KdfParams::low();
    assert_eq!(lo.m_cost_kib, 32768);
}
