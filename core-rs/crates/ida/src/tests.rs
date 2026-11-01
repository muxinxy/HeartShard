use super::*;

/// xorshift64 确定性伪随机（测试专用，不引入 rand 依赖）。
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

fn random_data(seed: u64, len: usize) -> Vec<u8> {
    let mut rng = XorShift(seed | 1);
    let mut data = vec![0u8; len];
    rng.fill(&mut data);
    data
}

/// 全组合随机丢失：从 n 个 shard 中随机保留 keep 个（含任意 data/recovery 混合）。
fn drop_to(shards: &[Vec<u8>], keep: Vec<usize>) -> Vec<(u8, Vec<u8>)> {
    keep.iter()
        .map(|&i| (i as u8, shards[i].clone()))
        .collect()
}

fn xorshift_pick(n: usize, keep: usize, seed: u64) -> Vec<usize> {
    let mut rng = XorShift(seed | 1);
    let mut idx: Vec<usize> = (0..n).collect();
    // Fisher-Yates 洗牌后取前 keep 个
    for i in (1..idx.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        idx.swap(i, j);
    }
    idx.truncate(keep);
    idx.sort_unstable();
    idx
}

#[test]
fn config_validation() {
    assert!(DispersalConfig::new(4, 2).is_ok());
    assert!(matches!(
        DispersalConfig::new(2, 4),
        Err(IdaError::InvalidConfig { n: 2, m: 4 })
    ));
    assert!(matches!(
        DispersalConfig::new(4, 0),
        Err(IdaError::InvalidConfig { .. })
    ));
    assert!(matches!(
        DispersalConfig::new(0, 0),
        Err(IdaError::InvalidConfig { .. })
    ));
    assert!(DispersalConfig::new(8, 8).is_ok()); // 无冗余：n == m
}

#[test]
fn roundtrip_n4_m2_loss_any_two() {
    let config = DispersalConfig::new(4, 2).unwrap();
    let data = random_data(0xDEADBEEF, 100_003); // 奇数长度，覆盖填充路径
    let shards = encode_shards(&data, config).unwrap();
    assert_eq!(shards.len(), 4);
    assert!(shards.iter().all(|s| s.len() % 2 == 0));

    // 保留任意 2 个（所有 C(4,2) 组合）都必须恢复
    for a in 0..4 {
        for b in (a + 1)..4 {
            let kept = drop_to(&shards, vec![a, b]);
            let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
            let restored = decode_shards(&refs, config, data.len()).unwrap();
            assert_eq!(restored, data, "loss pattern [{a}, {b}] must recover");
        }
    }

    // 只剩 1 个（m-1）必须失败
    for keep in 0..4 {
        let kept = drop_to(&shards, vec![keep]);
        let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
        assert!(
            matches!(
                decode_shards(&refs, config, data.len()),
                Err(IdaError::NotEnoughShards { .. })
            ),
            "keeping only shard {keep} must fail"
        );
    }
}

#[test]
fn roundtrip_n6_m3_random_losses() {
    let config = DispersalConfig::new(6, 3).unwrap();
    let data = random_data(0x1234_5678, 262_145); // 256KiB+1，覆盖多 shard + 填充
    let shards = encode_shards(&data, config).unwrap();
    assert_eq!(shards.len(), 6);

    for seed in 1..=8u64 {
        let keep = xorshift_pick(6, 3, seed);
        let kept = drop_to(&shards, keep.clone());
        let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
        let restored = decode_shards(&refs, config, data.len()).unwrap();
        assert_eq!(restored, data, "random loss {keep:?} (seed {seed}) must recover");
    }

    // m-1 = 2 个：全组合失败
    for a in 0..6 {
        for b in (a + 1)..6 {
            let kept = drop_to(&shards, vec![a, b]);
            let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
            assert!(decode_shards(&refs, config, data.len()).is_err());
        }
    }
}

#[test]
fn roundtrip_n8_m5_and_edge_configs() {
    for (n, m) in [(8u8, 5u8), (3, 2), (8, 8), (2, 1)] {
        let config = DispersalConfig::new(n, m).unwrap();
        let data = random_data(u64::from(n) * 31 + u64::from(m), 65_537);
        let shards = encode_shards(&data, config).unwrap();
        assert_eq!(shards.len(), usize::from(n));

        // 保留前 m 个 data shard（快路径）
        let keep_all_data: Vec<usize> = (0..usize::from(m)).collect();
        let kept = drop_to(&shards, keep_all_data);
        let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
        assert_eq!(decode_shards(&refs, config, data.len()).unwrap(), data);

        // 随机混合保留 m 个（走 RS 解码路径）
        let keep = xorshift_pick(usize::from(n), usize::from(m), 99);
        let kept = drop_to(&shards, keep);
        let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
        assert_eq!(decode_shards(&refs, config, data.len()).unwrap(), data);

        // m-1 必败
        let keep = xorshift_pick(usize::from(n), usize::from(m) - 1, 7);
        let kept = drop_to(&shards, keep);
        let refs: Vec<(u8, &[u8])> = kept.iter().map(|(i, s)| (*i, s.as_slice())).collect();
        assert!(decode_shards(&refs, config, data.len()).is_err(), "n={n}, m={m}");
    }
}

#[test]
fn empty_data_edge() {
    let config = DispersalConfig::new(4, 2).unwrap();
    let shards = encode_shards(&[], config).unwrap();
    assert_eq!(shards.len(), 4);
    assert!(shards.iter().all(|s| s.is_empty()));
    let refs: Vec<(u8, &[u8])> = shards.iter().enumerate().map(|(i, s)| (i as u8, s.as_slice())).collect();
    assert_eq!(decode_shards(&refs, config, 0).unwrap(), Vec::<u8>::new());
}

#[test]
fn corrupted_length_and_index_rejected() {
    let config = DispersalConfig::new(4, 2).unwrap();
    let data = random_data(42, 1000);
    let shards = encode_shards(&data, config).unwrap();

    // 长度不一致（截短非参照分片，参照 = 第一个传入分片）
    let mut bad = shards[1].clone();
    bad.truncate(bad.len() - 2);
    let refs: Vec<(u8, &[u8])> = vec![(0, &shards[0]), (1, &bad)];
    assert!(matches!(
        decode_shards(&refs, config, data.len()),
        Err(IdaError::ShardLengthMismatch(1, _, _))
    ));

    // 奇数长度
    let mut odd = shards[0].clone();
    odd.truncate(odd.len() - 1);
    let refs: Vec<(u8, &[u8])> = vec![(0, &odd), (1, &shards[1])];
    assert!(matches!(
        decode_shards(&refs, config, data.len()),
        Err(IdaError::OddShardLength(_))
    ));

    // 越界 index
    let refs: Vec<(u8, &[u8])> = vec![(0, &shards[0]), (9, &shards[1])];
    assert!(matches!(
        decode_shards(&refs, config, data.len()),
        Err(IdaError::DecodeFailed)
    ));
}

#[test]
fn one_mib_chunk_encoding_is_fast_and_correct() {
    // M0 规模冒烟：1 MiB chunk、n=4/m=2，全链路恢复
    let config = DispersalConfig::new(4, 2).unwrap();
    let data = random_data(0xC0FFEE, 1_048_576 + 5);
    let shards = encode_shards(&data, config).unwrap();
    let keep = xorshift_pick(4, 2, 3);
    let kept: Vec<(u8, &[u8])> = keep.iter().map(|&i| (i as u8, shards[i].as_slice())).collect();
    let restored = decode_shards(&kept, config, data.len()).unwrap();
    assert_eq!(hex_of_sha(restored.as_slice()), hex_of_sha(&data));
}

/// 简易 hash 十六进制（避免引 hex 依赖）。
fn hex_of_sha(data: &[u8]) -> String {
    // FNV-1a 64 足以区分测试数据
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
