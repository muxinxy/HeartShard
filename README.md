# HeartShard · 牧心相册

> **把记忆碎成片，散养在你信任的每一朵云上。**
>
> Shatter your memories into pieces — and let every cloud you trust raise them.

HeartShard 是一个**端到端加密**的家庭相册系统，覆盖 Windows / Android / Web 三端，
既可以**纯客户端**使用（无服务器、无账号，直连你自己的云存储），也可以连接
**自托管后台**获得多用户、网页端与分享链接能力。照片和视频被切碎、加密、
纠删冗余后分散到你自己的云存储（S3 兼容 / WebDAV / OpenList / FTPS）——
云存储与后台全程零知识。

HeartShard is an end-to-end encrypted family photo album for Windows, Android
and the Web. Use it **pure client-side** (no server, no accounts — talking
straight to your own cloud storage) or connect it to a **self-hosted server**
for multi-user, web access and share links. Photos/videos are sharded,
encrypted and scattered across your own S3-compatible / WebDAV / OpenList /
FTPS storage — the cloud and the server learn nothing.

## 核心特性 / Features

- 🔐 **端到端加密**：AES-256-GCM 逐 chunk 加密、Leopard-RS 纠删码（默认 n=4/m=2）
  冗余、SHA-256 全程校验；口令经 Argon2id 派生主密钥，绝不落盘
  *End-to-end encryption: per-chunk AES-256-GCM, Leopard-RS erasure coding,
  SHA-256 everywhere; password never leaves the device (local mode)*
- 🗂 **相册优先**：时间线、瀑布流网格、相册树、收藏夹、回收站（可恢复）、
  地图、按天/年月导航——加密与分片细节折叠在"高级详情"里
  *Album-first UI: timeline, grid, albums, favorites, restorable trash, map —
  crypto/sharding details tucked away*
- 🔗 **隐私分享**：相册或单张/多选照片生成链接，**默认去除 EXIF/GPS 元数据**，
  可选有效期与分享码；浏览器 WebCrypto 解密，服务器只见密文
  *Privacy sharing: album or multi-photo links with EXIF/GPS stripped by
  default, optional expiry & access code; WebCrypto decryption, the server
  only ever sees ciphertext*
- 👥 **多用户后台**（可选自托管）：argon2id 认证、相册级权限
  （隐藏/只看/上传/管理，继承+覆盖）、TOTP 两步验证、同步源管理
  *Optional self-hosted server: argon2id auth, album-level permissions
  (hidden/view/upload/manage with inheritance), TOTP 2FA, sync sources*
- 🔄 **多设备同步**：Lamport 时钟 + LWW 合并 + 增量 delta；同步工具
  （本地目录/云存储 → 相册，append/mirror 策略）
  *Multi-device sync: Lamport + LWW + delta; syncer CLI for local dirs and
  cloud sources (append/mirror strategies)*
- 🎞 **动态照片**：Android Motion Photo（JPEG 尾随 MP4）检测、提取、播放与
  双文件导出；Apple Live Photo 文件名配对
  *Motion photos: Android trailing-MP4 detection, extraction, playback and
  two-file export; Apple Live Photo filename pairing*
- ☁️ **多云分散**：同一 blob 的分片强制落在不同 Storage；S3（手写 SigV4）/
  WebDAV / OpenList（令牌）/ FTPS
  *Multi-cloud: shards of one blob land on different storages; S3
  (hand-written SigV4), WebDAV, OpenList (token), FTPS*

## 两种使用方式 / Two Ways to Use

### 1. 纯客户端（serverless）/ Pure client-side

桌面/安卓直接添加你的云存储，数据目录放本机——**无服务器、无账号**。
多设备共享 `mk.wrap` 主密钥文件即可同步。

Add your cloud storage in the desktop app; the data dir lives on your
machine — no server, no accounts. Share the `mk.wrap` key file across
devices for sync.

### 2. 连接自托管后台 / Self-hosted server

```bash
docker compose up -d        # 见仓库 docker-compose.yml；或直接跑 server 二进制
# 浏览器 http://NAS:8420 —— 多用户/相册权限/存储/同步源/分享全网页端管理
```

- 多用户 + 相册 ACL + TOTP 2FA + 分享链接
- 桌面客户端锁屏切「连接后台」即可接入；手机 PWA「添加到主屏幕」
- 详细配置见 [server.toml.example](server/server.toml.example) 与
  [开发文档](docs/development.md)

## 快速开始（开发）/ Quick start (dev)

```powershell
git clone <repo-url> heartshard; cd heartshard
mise install          # rust stable + node 24 + pnpm
cargo test --manifest-path core-rs/Cargo.toml --workspace
```

构建产物 / Build artifacts：

```powershell
cd apps\tauri; pnpm tauri build                 # Windows 便携 exe
.\scripts\build-android-apk.ps1                 # Android APK（arm64）
cd server; cargo build --release                # 后台服务二进制
docker compose up -d                            # 或容器化部署
```

更多细节见 [开发文档](docs/development.md) / [CHANGELOG](CHANGELOG.md)。

## 仓库结构 / Layout

```
heartshard/
├── core-rs/crates/             # Rust 核心：keypath / crypto / ida / shard / index
│   └── heart-core/             # 平台无关库：HeartCore + 云存储适配器
├── apps/tauri/                 # Windows + Android 客户端（Tauri 2 + React）
├── server/                     # 牧心后台（axum：多用户/ACL/同步/分享/PWA）
├── tools/syncer/               # 相册同步 CLI（append/mirror 策略）
├── scripts/                    # 构建脚本（Android APK 固化链）
├── docs/                       # 开发文档 / 威胁模型 / 算法研究
└── .github/workflows/          # 多平台多架构 CI
```

## 许可证 / License

[MIT](LICENSE)
