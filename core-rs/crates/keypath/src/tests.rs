use super::*;

const HASH: &str = "a3f1c2d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

#[test]
fn accepts_valid_segments() {
    for ok in ["a", "0", "_", "-", "abc123", "a3f1c2d4e5f6", "manifest.bin", "00000_00.shard", "x-y_z.9"] {
        assert_eq!(sanitize_key_segment(ok), Ok(ok), "should accept {ok:?}");
    }
}

#[test]
fn rejects_empty_and_too_long() {
    assert_eq!(sanitize_key_segment(""), Err(KeypathError::Empty));
    let max = "a".repeat(MAX_SEGMENT_BYTES);
    assert!(sanitize_key_segment(&max).is_ok());
    let over = "a".repeat(MAX_SEGMENT_BYTES + 1);
    assert!(matches!(
        sanitize_key_segment(&over),
        Err(KeypathError::TooLong { got: 256 })
    ));
}

#[test]
fn rejects_forbidden_chars() {
    // 简报 §5.2 禁用字符全集，以及大写/空白/Unicode/控制字符
    for bad in [
        '#', '*', ':', '?', '\'', '"', '<', '>', '|', '[', ']', '/', '\\',
        'A', 'Z', ' ', '\t', '\n', '\0', '\u{7f}',
        '中', '文', '😀', 'é', 'ß', '\u{00e9}',
    ] {
        let seg = format!("a{}b", bad);
        assert!(
            matches!(sanitize_key_segment(&seg), Err(KeypathError::ForbiddenChar(c)) if c == bad),
            "should reject {bad:?}"
        );
    }
}

#[test]
fn rejects_dot_edges_and_traversal() {
    assert!(matches!(sanitize_key_segment(".hidden"), Err(KeypathError::DotEdge)));
    assert!(matches!(sanitize_key_segment("trailing."), Err(KeypathError::DotEdge)));
    assert!(matches!(sanitize_key_segment("."), Err(KeypathError::DotEdge)));
    assert!(matches!(sanitize_key_segment(".."), Err(KeypathError::DotEdge)));
    // 内部点合法
    assert!(sanitize_key_segment("a.b.c").is_ok());
}

#[test]
fn rejects_windows_reserved_names_and_substrings() {
    // 精确命中
    for name in WINDOWS_RESERVED {
        assert!(
            matches!(sanitize_key_segment(name), Err(KeypathError::ReservedName(_))),
            "should reject reserved {name:?}"
        );
        // 带扩展名（Windows 语义下同样保留）
        let with_ext = format!("{name}.txt");
        assert!(matches!(sanitize_key_segment(&with_ext), Err(KeypathError::ReservedName(_))));
        // 子串命中（项目规范要求连子串都禁止）
        let substr = format!("my{name}file");
        assert!(matches!(sanitize_key_segment(&substr), Err(KeypathError::ReservedName(_))));
    }
    // 不含保留名的正常词不受影响（"con" 的子串误伤面由生成键不会含字母词规避）
    assert!(sanitize_key_segment("ab10ff").is_ok());
}

#[test]
fn blob_hash_validation() {
    assert_eq!(validate_blob_hash(HASH), Ok(HASH));
    assert!(matches!(validate_blob_hash(&HASH[..63]), Err(KeypathError::InvalidBlobHash(_))));
    assert!(matches!(
        validate_blob_hash(&HASH.to_uppercase()),
        Err(KeypathError::InvalidBlobHash(_))
    ));
    assert!(matches!(
        validate_blob_hash(&format!("{}g", &HASH[..63])),
        Err(KeypathError::InvalidBlobHash(_))
    ));
    assert!(matches!(validate_blob_hash(""), Err(KeypathError::InvalidBlobHash(_))));
}

#[test]
fn shard_key_format() {
    assert_eq!(
        shard_object_key(HASH, 0, 0).unwrap(),
        format!("blobs/a3/f1/{}/00000_00.shard", HASH)
    );
    assert_eq!(
        shard_object_key(HASH, 42, 3).unwrap(),
        format!("blobs/a3/f1/{}/00042_03.shard", HASH)
    );
    assert_eq!(
        shard_object_key(HASH, 123456, 255).unwrap(),
        format!("blobs/a3/f1/{}/123456_255.shard", HASH)
    );
    // 非法 hash 拒绝
    assert!(shard_object_key("A3", 0, 0).is_err());
    assert!(shard_object_key("zz", 0, 0).is_err());
}

#[test]
fn manifest_key_format() {
    assert_eq!(
        manifest_object_key(HASH).unwrap(),
        format!("manifests/a3/f1/{}/manifest.bin", HASH)
    );
    assert!(manifest_object_key(&HASH[..62]).is_err());
}

#[test]
fn index_key_format() {
    assert_eq!(
        index_object_key("device01ab", 42).unwrap(),
        "index/device01ab/42.idx"
    );
    assert_eq!(
        index_object_key("device01ab", 0).unwrap(),
        "index/device01ab/0.idx"
    );
    // deviceId 必须通过 sanitize
    assert!(index_object_key("CON", 1).is_err());
    assert!(index_object_key("", 1).is_err());
    assert!(index_object_key("dev/ice", 1).is_err());
}

/// 共享向量测试：与 `tools/keypath-tests/vectors.json` 对齐，
/// 其他端实现（ArkTS/JS）以同一文件为契约。
#[test]
fn shared_vectors_json() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tools/keypath-tests/vectors.json"
    );
    let raw = std::fs::read_to_string(path).expect("vectors.json must exist");
    let vectors: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");

    for case in vectors["sanitize"].as_array().expect("sanitize array") {
        let input = case["input"].as_str().expect("input str");
        let valid = case["valid"].as_bool().expect("valid bool");
        assert_eq!(
            sanitize_key_segment(input).is_ok(),
            valid,
            "sanitize({input:?}) expected valid={valid}"
        );
    }
    for case in vectors["shard_key"].as_array().expect("shard_key array") {
        let expected = case["expected"].as_str().expect("expected str");
        let got = shard_object_key(
            case["blob_hash"].as_str().unwrap(),
            case["chunk_index"].as_u64().unwrap() as u32,
            case["shard_index"].as_u64().unwrap() as u8,
        )
        .expect("shard key should be valid");
        assert_eq!(got, expected);
    }
    for case in vectors["manifest_key"].as_array().expect("manifest_key array") {
        let expected = case["expected"].as_str().expect("expected str");
        let got = manifest_object_key(case["blob_hash"].as_str().unwrap()).expect("valid");
        assert_eq!(got, expected);
    }
    for case in vectors["index_key"].as_array().expect("index_key array") {
        let expected = case["expected"].as_str().expect("expected str");
        let got = index_object_key(
            case["device_id"].as_str().unwrap(),
            case["lamport_clock"].as_u64().unwrap(),
        )
        .expect("valid");
        assert_eq!(got, expected);
    }
}
