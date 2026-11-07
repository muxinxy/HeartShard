//! 牧心后台 API 集成测试：登录 / ACL 继承与覆盖 / 上传 / 可见性过滤 / 同步。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use heart_core::library::HeartCore;
use heartshard_server::acl::AclStore;
use heartshard_server::api;
use heartshard_server::auth::UserStore;
use heartshard_server::config::{LibraryConfig, ServerConfig, UserEntry};
use heartshard_server::AppState;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tower::util::ServiceExt;

fn build_app(
    lib: &std::path::Path,
    root: &std::path::Path,
    sources: Vec<syncer::SourceConfig>,
) -> axum::Router {
    let mut core = HeartCore::new(lib);
    core.unlock("libpw").unwrap();

    let cfg = ServerConfig {
        port: 0,
        library: LibraryConfig {
            dir: lib.to_string_lossy().to_string(),
            password_env: "HS_UNUSED".into(),
        },
        users: vec![
            UserEntry {
                name: "alice".into(),
                password: Some("alicepw".into()),
                password_env: None,
                admin: true,
            },
            UserEntry {
                name: "bob".into(),
                password: Some("bobpw".into()),
                password_env: None,
                admin: false,
            },
            UserEntry {
                name: "eve".into(),
                password: Some("evepw".into()),
                password_env: None,
                admin: false,
            },
        ],
        groups: vec![],
        sources,
        sync_interval_secs: 0,
    };
    let users = UserStore::init(root, &cfg).unwrap();
    let acl = AclStore::load(root);
    let shares = heartshard_server::share::ShareStore::load(root);
    let state = Arc::new(AppState {
        core: Mutex::new(core),
        users,
        acl: Mutex::new(acl),
        cfg,
        server_root: root.to_path_buf(),
        shares: Mutex::new(shares),
    });
    api::router().with_state(state)
}

async fn json_body(res: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), 10 * 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn login(app: &axum::Router, name: &str, pw: &str) -> String {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "name": name, "password": pw }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "登录失败: {name}");
    let v = json_body(res).await;
    v["token"].as_str().unwrap().to_string()
}

async fn get(app: &axum::Router, uri: &str, token: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn auth_and_acl_and_upload() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = build_app(lib.path(), root.path(), vec![]);

    // ---- 认证 ----
    let alice = login(&app, "alice", "alicepw").await;
    let bob = login(&app, "bob", "bobpw").await;
    let eve = login(&app, "eve", "evepw").await;
    let bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "name": "alice", "password": "wrong" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    // ---- admin 建相册树：2024/旅行 ----
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/albums")
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "name": "2024" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let a2024 = json_body(res).await["id"].as_str().unwrap().to_string();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/albums")
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "name": "旅行", "parent_id": a2024 }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let a_trip = json_body(res).await["id"].as_str().unwrap().to_string();

    // bob（family 组）在根上无显式权限 → 全部相册不可见
    let res = get(&app, "/api/albums", &bob).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(json_body(res).await.as_array().unwrap().is_empty());

    // admin 给 family 组在 2024 设 view → bob 经继承可见 2024 和 旅行
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/albums/{a2024}/acl"))
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "principal": { "type": "group", "id": "family" },
                        "perm": "view"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = get(&app, "/api/albums", &bob).await;
    let albums = json_body(res).await;
    let names: Vec<&str> = albums
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"2024") && names.contains(&"旅行"), "{albums}");

    // bob 无 manage：建根相册被拒
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/albums")
                .header("authorization", format!("Bearer {bob}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "name": "x" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // ---- 上传（alice，manage）到 旅行 ----
    let png = {
        let mut buf = Vec::new();
        // 1x1 PNG（缩略图按需生成需要可解码图片）
        let img = image::DynamicImage::new_rgb8(1, 1);
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .unwrap();
        buf
    };
    let boundary = "XBOUND";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend_from_slice(&png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/albums/{a_trip}/upload"))
                .header("authorization", format!("Bearer {alice}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "上传失败");
    let up = json_body(res).await;
    assert_eq!(up["status"], "imported");
    let hash = up["blob_hash"].as_str().unwrap().to_string();

    // bob 能从时间线看到、能取原图；eve 显式 none 后不可见
    let res = get(&app, "/api/blobs", &bob).await;
    let blobs = json_body(res).await;
    assert_eq!(blobs.as_array().unwrap().len(), 1);

    let res = get(&app, &format!("/api/blobs/{hash}/raw"), &bob).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(bytes.as_ref(), png.as_slice(), "原图字节一致");

    // eve：显式 none 覆盖组 view
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/albums/{a2024}/acl"))
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "principal": { "type": "user", "id": "eve" },
                        "perm": "none"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = get(&app, "/api/albums", &eve).await;
    assert!(json_body(res).await.as_array().unwrap().is_empty(), "eve 应看不到 2024");
    let res = get(&app, &format!("/api/albums/{a_trip}/blobs"), &eve).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = get(&app, &format!("/api/blobs/{hash}/raw"), &eve).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sync_endpoint_imports_and_publishes() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();

    // 源目录放一张 1x1 PNG
    let png_path = src.path().join("backup/pic.png");
    std::fs::create_dir_all(png_path.parent().unwrap()).unwrap();
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    std::fs::write(&png_path, &png).unwrap();

    let sources = vec![syncer::SourceConfig {
        id: "test".into(),
        kind: "local".into(),
        root: Some(src.path().to_string_lossy().to_string()),
        strategy: "append".into(),
        delete_mode: "keep".into(),
        album_root: None,
        s3: None,
        webdav: None,
        openlist: None,
    }];
    let app = build_app(lib.path(), root.path(), sources);
    let alice = login(&app, "alice", "alicepw").await;

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync/run")
                .header("authorization", format!("Bearer {alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let report = json_body(res).await;
    assert_eq!(report[0]["added"], 1, "{report}");

    // 时间线出现照片（backup 相册可见——admin）
    let res = get(&app, "/api/blobs", &alice).await;
    assert_eq!(json_body(res).await.as_array().unwrap().len(), 1);
}

/// 分享流（M4）：创建分享 → meta 无密钥 → pkg 为密文 → blob_key（AES-GCM + AAD）
/// 可解出与上传一致的明文 —— 服务器只见密文的 E2E 语义验证。
#[tokio::test]
async fn share_e2e_ciphertext_roundtrip() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = build_app(lib.path(), root.path(), vec![]);
    let alice = login(&app, "alice", "alicepw").await;

    // 建相册 + 上传真实 PNG
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/albums")
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "name": "共享相册" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let album_id = json_body(res).await["id"].as_str().unwrap().to_string();

    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(64, 48)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let boundary = "XBOUND";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"share.png\"\r\n\r\n").as_bytes());
    body.extend_from_slice(&png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/albums/{album_id}/upload"))
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let up = json_body(res).await;
    assert_eq!(up["status"], "imported", "上传: {up}");
    let hash = up["blob_hash"].as_str().unwrap().to_string();

    // 创建分享
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/albums/{album_id}/share"))
                .header("authorization", format!("Bearer {alice}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "share 创建失败: {}", json_body(res).await);
    let share_id = json_body(res).await["share_id"].as_str().unwrap().to_string();

    // meta：文件清单存在，但绝不含密钥字段
    let res = get(&app, &format!("/s/{share_id}/meta"), "").await;
    assert_eq!(res.status(), StatusCode::OK);
    let meta = json_body(res).await;
    let file0 = &meta["files"][0];
    assert_eq!(file0["hash"], Value::String(hash.clone()));
    assert!(file0.get("blob_key").is_none() || file0["blob_key"].is_null(), "meta 不得下发密钥");
    let chunk_lens: Vec<u32> = file0["chunk_lens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();

    // 未登录用户可以拿到密文包（E2E：拿到密文拿不到明文）
    let res = get(&app, &format!("/s/{share_id}/pkg/{hash}"), "").await;
    assert_eq!(res.status(), StatusCode::OK);
    let pkg = axum::body::to_bytes(res.into_body(), 64 * 1024 * 1024).await.unwrap();
    assert_eq!(
        pkg.len() as u64,
        chunk_lens.iter().map(|l| *l as u64).sum::<u64>(),
        "密文长度 = Σchunk_lens"
    );

    // keys 端点（查看器取密钥的通道）+ Rust 侧走生产 decrypt_chunk 解密往返验证
    let res = get(&app, &format!("/s/{share_id}/keys"), "").await;
    let keys = json_body(res).await;
    let key_hex = keys["keys"][&hash].as_str().unwrap().to_string();
    let key_bytes: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&key_hex[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap();
    let hash_bytes: [u8; 32] = hex::decode(&hash).unwrap().try_into().unwrap();
    let mut plain = Vec::new();
    let mut off = 0usize;
    for (idx, len) in chunk_lens.iter().enumerate() {
        let chunk = &pkg[off..off + *len as usize];
        off += *len as usize;
        let pt = crypto::decrypt_chunk(
            &key_bytes,
            chunk,
            &hash_bytes,
            idx as u32,
            chunk_lens.len() as u32,
        )
        .expect("解密应成功（密文包 + 密钥可还原，服务器全程未见明文）");
        plain.extend_from_slice(&pt);
    }
    assert_eq!(plain, png, "解密明文与上传内容一致");
}
