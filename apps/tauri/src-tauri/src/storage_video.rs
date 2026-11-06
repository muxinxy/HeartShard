//! §10/§9 加密视频在线播放：自定义 URI 协议 `heartshard-video://`。
//!
//! 浏览器 <video> 元素发出 Range 请求，本协议处理器按需拉取覆盖区间的
//! 分片 → RS 重组 → 解密 → 返回明文字节 + Content-Range 头。
//! 流式播放不需要完整下载（简报 §10 在线播放加密视频）。
//!
//! URI 格式：`heartshard-video://localhost/{blob_hash}`
//! Range 请求自动由 <video> 元素发起（标准 HTTP Range 语义）。

use tauri::http::Request;
use tauri::http::Response;
use tauri::Manager;

use crate::commands::SessionState;

pub fn handle_video_request(
    ctx: &tauri::UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let state: tauri::State<SessionState> = ctx.app_handle().state();
    let core = state.core.lock().unwrap();

    let uri = request.uri().to_string();
    let blob_hash = uri
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    if blob_hash.len() != 64 {
        return response(400, b"bad blob hash".to_vec());
    }

    let range_header = request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let (offset, len) = parse_range(range_header).unwrap_or((0, u64::MAX));

    let Ok(detail) = core.blob_detail(&blob_hash) else {
        return response(404, b"blob not found".to_vec());
    };
    let total_len = detail.total_len as u64;
    let len = std::cmp::min(len, total_len.saturating_sub(offset));

    match core.export_blob_range(&blob_hash, offset, len) {
        Ok(data) => {
            let status = if range_header.is_empty() { 200 } else { 206 };
            let mut resp = Response::builder()
                .status(status)
                .header("Content-Type", "video/mp4")
                .header("Accept-Ranges", "bytes");
            if !range_header.is_empty() {
                resp = resp.header(
                    "Content-Range",
                    format!("bytes {}-{}/{}", offset, offset + len - 1, total_len),
                );
            }
            resp.body(data).unwrap_or_else(|_| response(500, Vec::new()))
        }
        Err(_) => response(500, b"decrypt failed".to_vec()),
    }
}

fn response(status: u16, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Length", body.len().to_string())
        .body(body)
        .unwrap_or_else(|_| Response::builder().status(500).body(Vec::new()).unwrap())
}

/// 解析 "bytes=start-end" 或 "bytes=start-" 格式。
fn parse_range(header: &str) -> Option<(u64, u64)> {
    let s = header.strip_prefix("bytes=")?;
    let start_str = s.split('-').next()?;
    let start: u64 = start_str.parse().ok()?;
    Some((start, u64::MAX))
}
