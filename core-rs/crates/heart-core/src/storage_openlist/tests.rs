use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// 极简 OpenList 兼容 mock：/api/auth/login、/api/fs/put、/d/<path>、
/// /api/fs/list（递归由适配器驱动，mock 只返回单层）、/api/fs/remove。
struct MockOpenList {
    addr: String,
    handle: Option<std::thread::JoinHandle<()>>,
}

type StoreMap = Arc<Mutex<HashMap<String, Vec<u8>>>>;

impl MockOpenList {
    fn start(files: StoreMap) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 65536];
                let mut raw: Vec<u8> = Vec::new();
                loop {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&buf[..n]);
                    if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head_end = raw
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                    .unwrap_or(raw.len());
                let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                let request_line = head.lines().next().unwrap_or_default().to_string();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let content_length: usize = head
                    .to_lowercase()
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse().ok()))
                    .unwrap_or(0);
                while raw.len() < head_end + content_length {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&buf[..n]);
                }
                let body_bytes = raw[head_end..].to_vec();
                let has_auth = head.to_lowercase().contains("authorization: heartshard-token");

                let json = |code: i64, msg: &str, data: &str| -> Vec<u8> {
                    format!("{{\"code\":{code},\"message\":\"{msg}\",\"data\":{data}}}").into_bytes()
                };

                let (status, resp_body): (&str, Vec<u8>) = if method == "POST" && target.contains("/api/auth/login") {
                    // 登录端点本身不带 token，必须在鉴权检查之前处理
                    ("200 OK", json(200, "ok", "{\"token\":\"heartshard-token\"}"))
                } else if !has_auth {
                    ("200 OK", json(401, "unauthorized", "null"))
                } else if method == "PUT" && target.contains("/api/fs/put") {
                    // File-Path 头：URL-encoded 全路径（hyper 发送小写头名，需大小写不敏感匹配）
                    let path = head
                        .to_lowercase()
                        .lines()
                        .find_map(|l| l.strip_prefix("file-path: "))
                        .map(percent_decode)
                        .unwrap_or_default();
                    files.lock().unwrap().insert(path, body_bytes);
                    ("200 OK", json(200, "ok", "null"))
                } else if method == "GET" && target.contains("/d/") {
                    // URL 归一化会吞掉连续斜杠：统一还原为带前导 / 的完整路径
                    let path = format!(
                        "/{}",
                        percent_decode(target.trim_start_matches("/d/")).trim_start_matches('/')
                    );
                    match files.lock().unwrap().get(&path) {
                        Some(v) => ("200 OK", v.clone()),
                        None => ("404 Not Found", b"not found".to_vec()),
                    }
                } else if method == "POST" && target.contains("/api/fs/list") {
                    let dir = parse_json_field(&String::from_utf8_lossy(&body_bytes), "path");
                    // 立即子项：文件 + 子目录（子目录让适配器递归下钻）
                    let mut children: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
                    for k in files.lock().unwrap().keys() {
                        if let Some(rest) = k.strip_prefix(&format!("{dir}/")) {
                            match rest.split_once('/') {
                                Some((seg, _)) => { children.insert(seg.to_string(), true); }
                                None => { children.insert(rest.to_string(), false); }
                            }
                        }
                    }
                    let entries = children
                        .iter()
                        .map(|(n, is_dir)| format!("{{\"name\":\"{n}\",\"is_dir\":{is_dir}}},"))
                        .collect::<String>();
                    let data = format!("{{\"content\":[{}]}}", entries.trim_end_matches(','));
                    ("200 OK", json(200, "ok", &data))
                } else if method == "POST" && target.contains("/api/fs/remove") {
                    let body = String::from_utf8_lossy(&body_bytes).to_string();
                    let dir = parse_json_field(&body, "dir");
                    let names_raw = parse_json_field(&body, "names");
                    for n in names_raw.split(',').filter(|s| !s.is_empty()) {
                        files.lock().unwrap().remove(&format!("{dir}/{n}"));
                    }
                    ("200 OK", json(200, "ok", "null"))
                } else {
                    ("404 Not Found", b"no route".to_vec())
                };

                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(&resp_body);
            }
        });
        Self { addr, handle: Some(handle) }
    }
}

impl Drop for MockOpenList {
    fn drop(&mut self) {
        drop(self.handle.take());
    }
}

fn parse_json_field(body: &str, field: &str) -> String {
    // 简化提取（仅 mock 用）："field":"value" 或 ["a","b"]
    let needle = format!("\"{field}\":");
    let Some(pos) = body.find(&needle) else { return String::new() };
    let rest = &body[pos + needle.len()..];
    if rest.starts_with('[') {
        let end = rest.find(']').unwrap_or(0);
        rest[1..end]
            .split(',')
            .map(|s| s.trim_matches('"').to_string())
            .collect::<Vec<_>>()
            .join(",")
    } else {
        let quoted = rest.trim_start().starts_with('"');
        if quoted {
            let end = rest[1..].find('"').unwrap_or(0);
            rest[1..1 + end].to_string()
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            rest[..end].to_string()
        }
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[test]
fn openlist_store_roundtrip_against_mock() {
    let files: StoreMap = Arc::new(Mutex::new(HashMap::new()));
    let mock = MockOpenList::start(files);

    let store = OpenListStore::new(OpenListConfig {
        base_url: format!("http://{}", mock.addr),
        username: "admin".into(),
        password: "pw".into(),
        base_path: "/heartshard".into(),
    })
    .unwrap();

    // put → get
    store.put("blobs/ab/cd/00000_00.shard", b"openlist-encrypted").unwrap();
    assert_eq!(
        store.get("blobs/ab/cd/00000_00.shard").as_deref(),
        Some(b"openlist-encrypted".as_slice())
    );
    assert_eq!(store.get("blobs/ab/cd/none.shard"), None);

    // list（嵌套目录递归）
    store.put("index/dev-a/1.idx", b"x").unwrap();
    store.put("index/dev-a/2.idx", b"y").unwrap();
    let keys = store.list("index").unwrap();
    assert_eq!(keys, vec!["index/dev-a/1.idx", "index/dev-a/2.idx"]);

    // delete 幂等
    store.delete("index/dev-a/1.idx").unwrap();
    assert_eq!(store.get("index/dev-a/1.idx"), None);
    store.delete("index/dev-a/1.idx").unwrap();
}

// 错误凭据的失败路径由真实 OpenList 实例验收（简化 mock 对任意登录都发 token）。
