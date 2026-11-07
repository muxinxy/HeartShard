use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// 极简 WebDAV 兼容 mock：PUT/GET/DELETE/PROPFIND(Depth1)/MKCOL。
struct MockDav {
    addr: String,
    _handle: std::thread::JoinHandle<()>,
}

impl MockDav {
    fn start(files: Arc<Mutex<HashMap<String, Vec<u8>>>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                        // 简化：读一次后按 Content-Length 补齐
                        let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                        let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
                        let clen: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().ok()))
                            .flatten()
                            .unwrap_or(0);
                        if raw.len() >= head_end + clen {
                            break;
                        }
                    }
                }
                let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
                let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or_default().to_string();
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let path = percent_decode(target.split('?').next().unwrap_or("/"));
                let body = raw[head_end + 4..].to_vec();

                let (status, resp_body) = if method == "PUT" {
                    files.lock().unwrap().insert(path, body);
                    ("201 Created", Vec::new())
                } else if method == "GET" {
                    match files.lock().unwrap().get(&path) {
                        Some(v) => ("200 OK", v.clone()),
                        None => ("404 Not Found", b"nope".to_vec()),
                    }
                } else if method == "DELETE" {
                    files.lock().unwrap().remove(&path);
                    ("204 No Content", Vec::new())
                } else if method == "MKCOL" {
                    ("201 Created", Vec::new())
                } else if method == "PROPFIND" {
                    let mut keys: Vec<String> = files
                        .lock()
                        .unwrap()
                        .keys()
                        .filter(|k| k.starts_with(&path) && k.len() > path.len())
                        .cloned()
                        .collect();
                    keys.sort();
                    let xml = format!(
                        "<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\">{}</D:multistatus>",
                        keys.iter()
                            .map(|k| format!("<D:response><D:href>{}</D:href></D:response>", k))
                            .collect::<String>()
                    );
                    ("207 Multi-Status", xml.into_bytes())
                } else {
                    ("400 Bad Request", b"unsupported".to_vec())
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    resp_body.len()
                );
                stream.write_all(resp.as_bytes()).unwrap();
                stream.write_all(&resp_body).unwrap();
            }
        });
        Self { addr, _handle: handle }
    }
}

#[test]
fn webdav_store_roundtrip_against_mock() {
    let files = Arc::new(Mutex::new(HashMap::new()));
    let mock = MockDav::start(files);

    let dav = WebDavStore::new(WebDavConfig {
        base_url: format!("http://{}/dav", mock.addr),
        username: "user".into(),
        password: "pass".into(),
    })
    .unwrap();

    // put → get
    dav.put("blobs/ab/cd/00000_00.shard", b"dav-encrypted").unwrap();
    assert_eq!(dav.get("blobs/ab/cd/00000_00.shard").as_deref(), Some(b"dav-encrypted".as_slice()));
    assert_eq!(dav.get("blobs/ab/cd/none.shard"), None);

    // list（PROPFIND 解析 + 前缀过滤）
    dav.put("index/dev-a/1.idx", b"x").unwrap();
    dav.put("index/dev-a/2.idx", b"y").unwrap();
    let keys = dav.list("index").unwrap();
    assert_eq!(keys, vec!["index/dev-a/1.idx", "index/dev-a/2.idx"]);

    // delete + 幂等
    dav.delete("index/dev-a/1.idx").unwrap();
    assert_eq!(dav.get("index/dev-a/1.idx"), None);
    dav.delete("index/dev-a/1.idx").unwrap();
}
