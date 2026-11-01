//! base62 编解码，逐位对齐 MixFile `BigIntBaseN`
//! （字母表 `0-9a-zA-Z`，前导 `0x00` 字节编码为前导 `'0'` 字符，数值部分最小位数）。

use crate::CryptoError;

const ALPHABET: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 编码：空输入 → 空串；前导零字节 → 前导 `'0'`。
pub fn encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let zcount = data.iter().take_while(|&&b| b == 0).count();
    let mut num: Vec<u8> = data[zcount..].to_vec(); // big-endian 大数
    let mut digits_rev: Vec<u8> = Vec::new();
    while num.iter().any(|&b| b != 0) {
        // 整体除以 62，余数为一位数字
        let mut rem: u32 = 0;
        for byte in num.iter_mut() {
            let cur = (rem << 8) | u32::from(*byte);
            *byte = (cur / 62) as u8;
            rem = cur % 62;
        }
        digits_rev.push(ALPHABET[rem as usize]);
    }
    let mut out = String::with_capacity(zcount + digits_rev.len());
    for _ in 0..zcount {
        out.push('0');
    }
    for d in digits_rev.into_iter().rev() {
        out.push(d as char);
    }
    out
}

fn digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(c - b'A') + 36),
        _ => None,
    }
}

/// 解码：`'0'` 前导 → 前导零字节；非法字符返回 [`CryptoError::Base62`]。
pub fn decode(s: &str) -> Result<Vec<u8>, CryptoError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let zcount = bytes.iter().take_while(|&&b| b == b'0').count();
    // 解析为以 32-bit limb 为基的大数（除法由 limb 数组完成，此处直接用乘加）
    let mut limbs: Vec<u32> = Vec::new(); // little-endian limbs, base 2^32
    for &c in &bytes[zcount..] {
        let v = digit_value(c).ok_or(CryptoError::Base62)?;
        // limbs = limbs * 62 + v
        let mut carry = u64::from(v);
        for limb in limbs.iter_mut() {
            let prod = u64::from(*limb) * 62 + carry;
            *limb = (prod & 0xffff_ffff) as u32;
            carry = prod >> 32;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
    }
    // limbs(little-endian) → big-endian bytes，去掉高位的零 limb
    let mut out = vec![0u8; zcount];
    while let Some(&top) = limbs.last() {
        if top == 0 && !limbs.is_empty() {
            limbs.pop();
        } else {
            break;
        }
    }
    if !limbs.is_empty() {
        let mut num_bytes = Vec::with_capacity(limbs.len() * 4);
        for limb in limbs.iter().rev() {
            num_bytes.extend_from_slice(&limb.to_be_bytes());
        }
        // 去掉数值最高字节前的零（首 limb 的高位零）
        let first_nonzero = num_bytes.iter().position(|&b| b != 0).unwrap_or(num_bytes.len() - 1);
        out.extend_from_slice(&num_bytes[first_nonzero..]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_leading_zero() {
        // 0x00 保留为前导 '0'（对齐 BigIntBaseN）
        let data = [
            0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let enc = encode(&data);
        assert!(enc.starts_with('0'), "leading 0x00 must encode as leading '0', got {enc:?}");
        assert_eq!(decode(&enc).unwrap(), data);
    }

    #[test]
    fn roundtrip_no_leading_zero() {
        let data = [0x12u8, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
        let enc = encode(&data);
        assert!(!enc.starts_with('0'));
        assert_eq!(decode(&enc).unwrap(), data);
    }

    #[test]
    fn empty_and_zero_edges() {
        assert_eq!(encode(&[]), "");
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
        assert_eq!(encode(&[0x00]), "0");
        assert_eq!(decode("0").unwrap(), vec![0x00u8]);
        assert_eq!(encode(&[0x00, 0x00]), "00");
        assert_eq!(decode("00").unwrap(), vec![0x00u8, 0x00]);
    }

    #[test]
    fn rejects_invalid_chars() {
        assert!(decode("0-9").is_err());
        assert!(decode("中").is_err());
        assert!(decode("0_").is_err()); // '_' 不在字母表
        // 注意：MixFile base62 不排除易混淆字符——'O'、'l'、'I' 都是合法位
        assert!(decode("0O").is_ok());
        assert!(decode("0l").is_ok());
    }

    #[test]
    fn known_value() {
        // 255 = "3Z"（base62）：3*62+9=195? 验证：'3'=3, 'Z'=61 → 3*62+61=247≠255。
        // 255 = 4*62+7 → "47"（'4'=4, '7'=7）
        assert_eq!(encode(&[0xff]), "47");
        assert_eq!(decode("47").unwrap(), vec![0xffu8]);
        // 0x0100 = 256 = 4*62+8 → "48"
        assert_eq!(encode(&[0x01, 0x00]), "48");
    }
}
