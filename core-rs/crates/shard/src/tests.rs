use super::*;
use crypto::{generate_iv, DerivedKeys, KdfParams};

/// xorshift64 确定性伪随机（测试专用）。
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
}

fn random_blob(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = XorShift(seed | 1);
    let mut data = vec![0u8; len];
    rng.fill(&mut data);
    data
}

/// 确定性 IV 序列（测试注入；生产用 generate_iv）。
fn fixed_iv_stream(mut start: u8) -> impl FnMut() -> Result<[u8; IV_LEN], CryptoError> {
    move || {
        let iv = [start; IV_LEN];
        start = start.wrapping_add(1);
        Ok(iv)
    }
}

/// 把 shard 按 shard_index 模 2 分配到两个模拟 Storage。
fn place_on_storages(encoded: &EncodedBlob) -> [Vec<(u32, u8, Vec<u8>)>; 2] {
    let mut storages: [Vec<(u32, u8, Vec<u8>)>; 2] = Default::default();
    for s in &encoded.shards {
        let target = usize::from(s.shard_index) % 2;
        storages[target].push((s.chunk_index, s.shard_index, s.data.clone()));
    }
    storages
}

fn fetch_from<'a>(
    storages: &'a [Vec<(u32, u8, Vec<u8>)>],
    enabled: &'a [bool],
) -> impl FnMut(u32, u8) -> Option<Vec<u8>> + 'a {
    move |chunk_index, shard_index| {
        let target = usize::from(shard_index) % 2;
        if !enabled[target] {
            return None;
        }
        storages[target]
            .iter()
            .find(|(c, s, _)| *c == chunk_index && *s == shard_index)
            .map(|(_, _, d)| d.clone())
    }
}

/// 公共编码助手：派生 blob_key 并编码。
fn encode_test_blob(blob: &[u8], chunk_size: usize, n: u8, m: u8) -> (EncodedBlob, [u8; 32]) {
    let mk = crypto::derive_master_key(b"test-password", b"test-salt-16byte", &KdfParams::low())
        .unwrap();
    let keys = DerivedKeys::new(mk, b"heartshard-master-salt-v1").unwrap();
    let blob_hash = crypto::sha256(blob);
    let blob_key: [u8; 32] = *keys.blob_key(&blob_hash).unwrap();
    let encoded = encode_blob(blob, &blob_key, chunk_size, n, m, fixed_iv_stream(0xA0)).unwrap();
    (encoded, blob_key)
}

fn chunk_infos(encoded: &EncodedBlob) -> Vec<ChunkManifestInfo> {
    encoded
        .chunks
        .iter()
        .map(|c| ChunkManifestInfo {
            plaintext_sha256: c.plaintext_sha256,
            ciphertext_len: c.ciphertext.len(),
        })
        .collect()
}

#[test]
fn chunk_size_validation() {
    assert!(validate_chunk_size(DEFAULT_CHUNK_SIZE).is_ok());
    assert!(validate_chunk_size(MIN_CHUNK_SIZE).is_ok());
    assert!(validate_chunk_size(MAX_CHUNK_SIZE).is_ok());
    assert!(matches!(
        validate_chunk_size(MIN_CHUNK_SIZE - 1),
        Err(ShardError::InvalidChunkSize { .. })
    ));
    assert!(matches!(
        validate_chunk_size(MAX_CHUNK_SIZE + 1),
        Err(ShardError::InvalidChunkSize { .. })
    ));
}

#[test]
fn encode_structure_and_keys() {
    let blob = random_blob(1, (DEFAULT_CHUNK_SIZE * 5) / 2); // 2.5 MiB → 3 chunks
    let (encoded, _) = encode_test_blob(&blob, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M);
    assert_eq!(encoded.chunks.len(), 3);
    assert_eq!(encoded.shards.len(), 12); // 3 chunks × n=4
    assert_eq!(encoded.total_len, blob.len());
    assert_eq!(encoded.blob_sha256, crypto::sha256(&blob));

    // chunk 边界：前两片满、末片 0.5 MiB
    let lens: Vec<usize> = encoded
        .chunks
        .iter()
        .map(|c| c.ciphertext.len() - IV_LEN - crypto::TAG_LEN)
        .collect();
    assert_eq!(lens, vec![DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_SIZE / 2]);

    // 对象键与 shard hash 一致
    let blob_hex = {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::new();
        for b in encoded.blob_sha256 {
            s.push(HEX[usize::from(b >> 4)] as char);
            s.push(HEX[usize::from(b & 0x0f)] as char);
        }
        s
    };
    for s in &encoded.shards {
        let expected = keypath::shard_object_key(&blob_hex, s.chunk_index, s.shard_index).unwrap();
        assert_eq!(s.object_key, expected);
        assert_eq!(s.sha256, crypto::sha256(&s.data));
    }
}

#[test]
fn roundtrip_full_blob() {
    for len in [0usize, 1, MIN_CHUNK_SIZE - 1, MIN_CHUNK_SIZE, MIN_CHUNK_SIZE + 1, DEFAULT_CHUNK_SIZE * 3 + 7] {
        let blob = random_blob(len as u64 + 11, len);
        let (encoded, blob_key) = encode_test_blob(&blob, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M);
        let infos = chunk_infos(&encoded);
        let decoded = decode_blob(
            &encoded.blob_sha256,
            &blob_key,
            &infos,
            DEFAULT_N,
            DEFAULT_M,
            |chunk, shard| {
                encoded
                    .shards
                    .iter()
                    .find(|s| s.chunk_index == chunk && s.shard_index == shard)
                    .map(|s| s.data.clone())
            },
        )
        .unwrap_or_else(|e| panic!("len={len}: {e}"));
        assert_eq!(decoded, blob, "roundtrip len={len}");
        assert_eq!(crypto::sha256(&decoded), encoded.blob_sha256);
    }
}

#[test]
fn storage_loss_recovery_end_to_end() {
    // M1 冗余场景预演：shard 分散到 2 个 Storage，整个 Storage-0 丢失后仍可恢复
    let blob = random_blob(0xFEED, DEFAULT_CHUNK_SIZE * 2 + 12345);
    let (encoded, blob_key) = encode_test_blob(&blob, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M);
    let infos = chunk_infos(&encoded);
    let storages = place_on_storages(&encoded);

    // Storage-0 在线：全量可解
    let ok = decode_blob(
        &encoded.blob_sha256,
        &blob_key,
        &infos,
        DEFAULT_N,
        DEFAULT_M,
        fetch_from(&storages, &[true, true]),
    );
    assert_eq!(ok.unwrap(), blob);

    // Storage-0 整体丢失：只剩 shard_index 1,3（每 chunk 恰好 2 = m）→ 仍可解
    let degraded = decode_blob(
        &encoded.blob_sha256,
        &blob_key,
        &infos,
        DEFAULT_N,
        DEFAULT_M,
        fetch_from(&storages, &[false, true]),
    );
    assert_eq!(degraded.unwrap(), blob, "must survive total loss of one storage");

    // 两个 Storage 都丢：MissingShards
    let dead = decode_blob(
        &encoded.blob_sha256,
        &blob_key,
        &infos,
        DEFAULT_N,
        DEFAULT_M,
        fetch_from(&storages, &[false, false]),
    );
    assert!(matches!(
        dead,
        Err(ShardError::MissingShards { got: 0, need: 2, .. })
    ));
}

#[test]
fn corrupted_shard_detected_by_integrity_chain() {
    // 坏 shard（内容翻转）→ RS 重组 → GCM 失败 → ChunkCorrupted（M5 修复任务的原语）
    let blob = random_blob(0xBAD, DEFAULT_CHUNK_SIZE + 999);
    let (mut encoded, blob_key) = encode_test_blob(&blob, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M);
    let infos = chunk_infos(&encoded);
    encoded.shards[0].data[0] ^= 0xFF;
    let corrupted = encoded.shards.clone();
    let result = decode_blob(
        &encoded.blob_sha256,
        &blob_key,
        &infos,
        DEFAULT_N,
        DEFAULT_M,
        |chunk, shard| {
            corrupted
                .iter()
                .find(|s| s.chunk_index == chunk && s.shard_index == shard)
                .map(|s| s.data.clone())
        },
    );
    assert!(
        matches!(result, Err(ShardError::ChunkCorrupted(0))),
        "corrupted shard must surface as ChunkCorrupted, got {result:?}"
    );
}

#[test]
fn per_chunk_plaintext_hash_recorded() {
    let blob = random_blob(0x5A5A, DEFAULT_CHUNK_SIZE / 2);
    let (encoded, _) = encode_test_blob(&blob, DEFAULT_CHUNK_SIZE, DEFAULT_N, DEFAULT_M);
    assert_eq!(encoded.chunks.len(), 1);
    assert_eq!(
        encoded.chunks[0].plaintext_sha256,
        crypto::sha256(&blob),
        "chunk plaintext hash == blob plaintext hash (single chunk)"
    );
}

#[test]
fn random_iv_stream_produces_distinct_ivs() {
    // 生产路径：generate_iv 连续产生不同 IV（统计性抽查）
    let ivs: Vec<[u8; IV_LEN]> = (0..8).map(|_| generate_iv().unwrap()).collect();
    for i in 0..ivs.len() {
        for j in (i + 1)..ivs.len() {
            assert_ne!(ivs[i], ivs[j], "random IVs must differ (probabilistic)");
        }
    }
}
