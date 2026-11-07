use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// AWS 官方文档向量（S3 SigV4 header-based auth, GET Object 示例）。
#[test]
fn aws_documented_get_object_vector() {
    // 官方文档给出的 canonical request（逐字）
    let canonical_request = format!(
        "GET\n/test.txt\n\nhost:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:{EMPTY_SHA}\nx-amz-date:20130524T000000Z\n\nhost;range;x-amz-content-sha256;x-amz-date\n{EMPTY_SHA}"
    );
    let signature = sign_canonical(
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        "20130524",
        "us-east-1",
        "20130524T000000Z",
        &canonical_request,
    );
    assert_eq!(
        signature,
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
        "must match the AWS documented signature"
    );
}

#[test]
fn canonical_query_sorted_and_encoded() {
    let pairs = vec![
        ("prefix".to_string(), "index/".to_string()),
        ("list-type".to_string(), "2".to_string()),
        ("continuation-token".to_string(), "a/b+c d".to_string()),
    ];
    let q = canonical_query(&pairs).unwrap();
    // 字典序：continuation-token < list-type < prefix；'/' 与 '+' 与空格编码
    assert_eq!(q, "continuation-token=a%2Fb%2Bc%20d&list-type=2&prefix=index%2F");
}

#[test]
fn amz_dates_known_timestamp() {
    // 2013-05-24T00:00:00Z = 1369353600（15849 天）
    let (amz, date) = amz_dates(UNIX_EPOCH + std::time::Duration::from_secs(1_369_353_600)).unwrap();
    assert_eq!(amz, "20130524T000000Z");
    assert_eq!(date, "20130524");
}

#[test]
fn encode_path_keeps_slash_encodes_rest() {
    assert_eq!(encode_path("blobs/ab/cd/00000_00.shard").unwrap(), "blobs/ab/cd/00000_00.shard");
    // AWS 规则：路径中 '/' 保留，其余非 unreserved 编码
    assert_eq!(encode_path("a b/c+d").unwrap(), "a%20b/c%2Bd");
}

// ---------------------------------------------------------------------------
// 本地 mock S3 server 集成测试（验证适配器管线：签名头存在、put/get/list/delete）
// ---------------------------------------------------------------------------

type StoreMap = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// 顺序应答的极简 S3 兼容服务器（仅本测试使用）。
struct MockS3 {
    addr: String,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockS3 {
    fn start(store: StoreMap) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 65536];

                // 读完整请求头
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

                // 按 Content-Length 补齐 body
                while raw.len() < head_end + content_length {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&buf[..n]);
                }
                let body_bytes = raw[head_end..].to_vec();

                let has_auth = head.to_lowercase().contains("authorization: aws4-hmac-sha256");
                let has_date = head.to_lowercase().contains("x-amz-date:");
                // path-style：剥掉首段 bucket 名得到对象 key
                let key = target
                    .trim_start_matches('/')
                    .split_once('/')
                    .map(|(_, k)| k.to_string())
                    .unwrap_or_default();

                let (status, resp_body): (&str, Vec<u8>) = if !has_auth || !has_date {
                    ("401 Unauthorized", b"missing auth".to_vec())
                } else if method == "PUT" {
                    store.lock().unwrap().insert(key, body_bytes);
                    ("200 OK", Vec::new())
                } else if method == "GET" && target.contains("list-type=2") {
                    let prefix = target
                        .split("prefix=")
                        .nth(1)
                        .map(|p| p.split('&').next().unwrap_or("").to_string())
                        .unwrap_or_default();
                    let decoded = prefix.replace("%2F", "/");
                    let mut keys: Vec<String> = store
                        .lock()
                        .unwrap()
                        .keys()
                        .filter(|k| k.starts_with(&decoded))
                        .cloned()
                        .collect();
                    keys.sort();
                    let xml = format!(
                        "<?xml version=\"1.0\"?><ListBucketResult>{}</ListBucketResult>",
                        keys.iter().map(|k| format!("<Key>{k}</Key>")).collect::<String>()
                    );
                    ("200 OK", xml.into_bytes())
                } else if method == "GET" {
                    match store.lock().unwrap().get(&key) {
                        Some(v) => ("200 OK", v.to_vec()),
                        None => ("404 Not Found", b"no such key".to_vec()),
                    }
                } else if method == "DELETE" {
                    store.lock().unwrap().remove(&key);
                    ("204 No Content", Vec::new())
                } else {
                    ("400 Bad Request", b"unsupported".to_vec())
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

impl Drop for MockS3 {
    fn drop(&mut self) {
        drop(self.handle.take());
    }
}

#[test]
fn s3_store_roundtrip_against_mock() {
    let store_map: StoreMap = Arc::new(Mutex::new(HashMap::new()));
    let mock = MockS3::start(store_map);

    let s3 = S3Store::new(S3Config {
        endpoint: format!("http://{}", mock.addr),
        bucket: "heartshard-test".into(),
        access_key: "AKIAIOSFODNN7EXAMPLE".into(),
        secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        region: "us-east-1".into(),
        path_style: true,
    })
    .unwrap();
    use index::store::BlobStore as _;

    // put → get 逐字节
    s3.put("blobs/ab/cd/00000_00.shard", b"encrypted-bytes").unwrap();
    assert_eq!(s3.get("blobs/ab/cd/00000_00.shard").as_deref(), Some(b"encrypted-bytes".as_slice()));
    assert_eq!(s3.get("blobs/ab/cd/99999_99.shard"), None, "404 must map to None");

    // list（prefix 过滤）
    s3.put("index/dev-a/1.idx", b"x").unwrap();
    s3.put("index/dev-a/2.idx", b"y").unwrap();
    let keys = s3.list("index").unwrap();
    assert_eq!(keys, vec!["index/dev-a/1.idx", "index/dev-a/2.idx"]);

    // delete + 幂等
    s3.delete("index/dev-a/1.idx").unwrap();
    assert_eq!(s3.get("index/dev-a/1.idx"), None);
    s3.delete("index/dev-a/1.idx").unwrap(); // 404 → Ok

    // 体积上限内的较大对象
    let big = vec![0xABu8; 200_000];
    s3.put("blobs/ff/ee/big.bin", &big).unwrap();
    assert_eq!(s3.get("blobs/ff/ee/big.bin").as_deref(), Some(big.as_slice()));
}
