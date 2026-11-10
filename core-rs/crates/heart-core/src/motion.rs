//! 动态照片检测与提取（M5c）：
//! - **Android 动态照片**：JPEG 主图尾随一个 MP4 视频（Google "Motion Photo" /
//!   三星 "Motion Photo"）。检测特征：JPEG 扫描 XMP GCamera 命名空间
//!   （`MicroVideo`/`MotionPhoto`）标记 + 找到尾随 `ftyp` box 的 MP4 头。
//!   提取 = 从 MP4 头字节切分出尾随视频段。
//! - **Apple Live Photo**：HEIC/JPEG 主图 + 独立 MOV，通过 ContentIdentifier
//!   EXIF 配对——导入时同目录同名（去扩展名）配对即可（ContentIdentifier 深度
//!   解析属 HEIC 元数据，v1 降级为文件名配对，如实标注）。
//!
//! 检测是纯字节操作，无 C 依赖；提取出的视频段按普通视频 blob 走既有
//! 加密分片管线，主图记录 `motion_video_hash` 关联。

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MotionInfo {
    /// 是否检测为动态照片
    pub is_motion: bool,
    /// 尾随视频段的起始偏移（字节）；None = 未检测到
    pub video_offset: Option<usize>,
    /// 视频段格式（mp4/mov）
    pub video_format: &'static str,
}

/// MP4/MOV 家族的 ftyp box 签名（box 大小 4B + "ftyp" 4B）。
const FTYP_SIGNATURES: [&[u8]; 4] = [
    b"ftypisom", // 通用 MP4
    b"ftypmp42", // MP4 v2
    b"ftypqt  ", // Apple QuickTime MOV
    b"ftypMSNV", // Sony/相机 MP4
];

/// JPEG SOI/EOI 标记。
const JPEG_SOI: [u8; 2] = [0xFF, 0xD8];
const JPEG_EOI: [u8; 2] = [0xFF, 0xD9];

/// 检测 JPEG 是否为动态照片，并定位尾随视频。
/// 策略（对 MT Photos/主流实现的对齐）：
/// 1. 从文件尾部向前搜索 `ftyp` 签名（尾随视频头）；
/// 2. 校验主图 XMP 含 GCamera/MotionPhoto/MicroVideo 标记（防止普通 JPEG
///    后附随机字节误判）；
/// 3. `ftyp` 位置即视频起始。
pub fn detect_jpeg_motion(data: &[u8]) -> MotionInfo {
    let mut info = MotionInfo {
        is_motion: false,
        video_offset: None,
        video_format: "mp4",
    };
    if data.len() < 4 || data[0..2] != JPEG_SOI {
        return info;
    }
    // XMP 标记检查（在 JPEG APP1 段内找 GCamera/MotionPhoto 关键字）
    let has_marker = find_subslice(data, b"http://ns.google.com/photos/1.0/camera/")
        .is_some()
        || find_subslice(data, b"MotionPhoto").is_some()
        || find_subslice(data, b"MicroVideo").is_some();
    if !has_marker {
        return info;
    }
    // 尾部向前找 ftyp 签名（取最后一个 = 最靠近尾部）
    for sig in FTYP_SIGNATURES.iter() {
        if let Some(pos) = rfind_subslice(data, sig) {
            // ftyp 前还有 4 字节 box size；视频段从 box size 开始
            let start = pos.saturating_sub(4);
            info.is_motion = true;
            info.video_offset = Some(start);
            info.video_format = if sig == &b"ftypqt  " { "mov" } else { "mp4" };
            return info;
        }
    }
    info
}

/// 从 JPEG 字节中提取尾随视频段（含 ftyp box 头）。
pub fn extract_motion_video(data: &[u8], info: &MotionInfo) -> Option<Vec<u8>> {
    let start = info.video_offset?;
    if start >= data.len() {
        return None;
    }
    Some(data[start..].to_vec())
}

/// 文件名配对（Apple Live Photo v1）：IMG_1234.HEIC ↔ IMG_1234.MOV。
/// 返回配对的视频路径（若存在且扩展名合法）。
pub fn find_live_photo_pair(image_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let stem = image_path.file_stem()?.to_str()?;
    let parent = image_path.parent()?;
    for ext in ["mov", "MOV", "Mov", "mp4", "MP4"] {
        let candidate = parent.join(format!("{stem}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成最小动态照片：JPEG（SOI+EOI）+ XMP 标记 + 尾随 MP4 头。
    fn synthetic_motion_jpeg() -> Vec<u8> {
        let mut data = vec![0xFF, 0xD8]; // SOI
        data.extend_from_slice(b"http://ns.google.com/photos/1.0/camera/ MicroVideo=1");
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        // 尾随 MP4：4B size + ftypisom
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x18]);
        data.extend_from_slice(b"ftypisom");
        data.extend_from_slice(b"fake-video-payload");
        data
    }

    #[test]
    fn detects_and_extracts_trailing_mp4() {
        let data = synthetic_motion_jpeg();
        let info = detect_jpeg_motion(&data);
        assert!(info.is_motion);
        assert_eq!(info.video_format, "mp4");
        let video = extract_motion_video(&data, &info).unwrap();
        // 视频段以 ftyp box 开头（含 4B size）
        assert!(video.starts_with(&[0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p']));
        assert!(video.ends_with(b"fake-video-payload"));
    }

    #[test]
    fn plain_jpeg_not_motion() {
        let mut data = vec![0xFF, 0xD8];
        data.extend_from_slice(b"just a normal jpeg");
        data.extend_from_slice(&[0xFF, 0xD9]);
        let info = detect_jpeg_motion(&data);
        assert!(!info.is_motion);
        assert!(info.video_offset.is_none());
    }

    #[test]
    fn jpeg_with_marker_but_no_mp4_not_motion() {
        let mut data = vec![0xFF, 0xD8];
        data.extend_from_slice(b"http://ns.google.com/photos/1.0/camera/ MicroVideo=1");
        data.extend_from_slice(&[0xFF, 0xD9]);
        let info = detect_jpeg_motion(&data);
        assert!(!info.is_motion, "有 XMP 标记但无尾随 MP4 → 不算动态照片");
    }

    #[test]
    fn quicktime_format_detected() {
        let mut data = vec![0xFF, 0xD8];
        data.extend_from_slice(b"MotionPhoto");
        data.extend_from_slice(&[0xFF, 0xD9]);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]);
        data.extend_from_slice(b"ftypqt  ");
        data.extend_from_slice(b"payload");
        let info = detect_jpeg_motion(&data);
        assert!(info.is_motion);
        assert_eq!(info.video_format, "mov");
    }

    #[test]
    fn live_photo_pairing() {
        let dir = tempfile::tempdir().unwrap();
        let heic = dir.path().join("IMG_0001.HEIC");
        let mov = dir.path().join("IMG_0001.MOV");
        std::fs::write(&heic, b"fake").unwrap();
        std::fs::write(&mov, b"fake-video").unwrap();
        let paired = find_live_photo_pair(&heic);
        // Windows 文件系统不区分大小写：先尝试的 "mov" 即命中，路径等同即可
        assert!(paired.is_some(), "应配对到同 stem 的 MOV");
        let paired_path = paired.unwrap();
        assert_eq!(
            paired_path.file_stem().and_then(|s| s.to_str()),
            Some("IMG_0001")
        );
        assert!(paired_path.to_str().unwrap().to_lowercase().ends_with(".mov"));
        // 无配对
        let alone = dir.path().join("IMG_0002.HEIC");
        std::fs::write(&alone, b"fake").unwrap();
        assert_eq!(find_live_photo_pair(&alone), None);
    }
}
