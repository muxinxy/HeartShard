<div align="center">

<img src="apps/tauri/src-tauri/icons/icon.png" width="128" alt="牧心相册图标"/>

# 牧心相册 HeartShard

**把记忆碎成片，散养在你信任的每一朵云上。**

[简体中文](README.md) | [English](README.en.md)

</div>

牧心相册是一个**端到端加密**的家庭相册系统，覆盖 Windows / Android / Web 三端，
既可以**纯客户端**使用（无服务器、无账号，直连你自己的云存储），也可以连接
**自托管后台**获得多用户、网页端与分享链接能力。照片和视频被切碎、加密、
纠删冗余后分散到你自己的云存储（S3 兼容 / WebDAV / OpenList / FTPS）——
云存储与后台全程零知识。

## 目录

- [核心特性](#核心特性)
- [两种使用方式](#两种使用方式)
- [Docker 部署](#docker-部署)
- [快速开始（开发）](#快速开始开发)
- [仓库结构](#仓库结构)
- [许可证](#许可证)

## 核心特性

- **端到端加密**：AES-256-GCM 逐 chunk 加密、Leopard-RS 纠删码（默认 n=4/m=2）
  冗余、SHA-256 全程校验；口令经 Argon2id 派生主密钥，绝不落盘
- **相册优先**：时间线、瀑布流网格、相册树、收藏夹、回收站（可恢复）、
  地图、按天/年月导航——加密与分片细节折叠在"高级详情"里
- **隐私分享**：相册或单张/多选照片生成链接，**默认去除 EXIF/GPS 元数据**，
  可选有效期与分享码；浏览器 WebCrypto 解密，服务器只见密文
- **多用户后台**（可选自托管）：argon2id 认证、相册级权限
  （隐藏/只看/上传/管理，继承+覆盖）、TOTP 两步验证、同步源管理
- **多设备同步**：Lamport 时钟 + LWW 合并 + 增量 delta；同步工具
  （本地目录/云存储 → 相册，append/mirror 策略）
- **动态照片**：Android Motion Photo（JPEG 尾随 MP4）检测、提取、播放与
  双文件导出；Apple Live Photo 文件名配对
- **多云分散**：同一 blob 的分片强制落在不同存储；S3（手写 SigV4）/
  WebDAV / OpenList（令牌）/ FTPS

## 两种使用方式

### 1. 纯客户端（serverless）

桌面/安卓直接添加你的云存储，数据目录放本机——**无服务器、无账号**。
多设备共享 `mk.wrap` 主密钥文件即可同步。

### 2. 连接自托管后台

- 多用户 + 相册权限 + TOTP 两步验证 + 分享链接
- 桌面客户端锁屏切「连接后台」即可接入；手机浏览器打开后台地址可加为 PWA
- 详细配置见 [server.toml.example](server/server.toml.example) 与
  [开发文档](docs/development.md)

## Docker 部署

后台服务提供多架构镜像（amd64 / arm64 / armv7）：

```bash
docker pull ghcr.io/muxinxy/heartshard-server:v0.1.0

docker run -d -p 8420:8420 \
  -v heartshard-lib:/data/library \
  -v heartshard-meta:/data/meta \
  -e HEARTSHARD_LIB_PW=你的相册库口令 \
  ghcr.io/muxinxy/heartshard-server:v0.1.0
```

或用 [docker-compose.yml](docker-compose.yml)：改 `.env` 里的
`HEARTSHARD_LIB_PW` 后 `docker compose up -d`，浏览器访问
`http://NAS:8420`。

## 快速开始（开发）

```powershell
git clone https://github.com/muxinxy/HeartShard.git heartshard; cd heartshard
mise install          # rust stable + node 24 + pnpm
cargo test --manifest-path core-rs/Cargo.toml --workspace
```

构建产物：

```powershell
cd apps\tauri; pnpm tauri build                 # Windows 便携 exe
.\scripts\build-android-apk.ps1                 # Android APK（arm64）
cd server; cargo build --release                # 后台服务二进制
```

更多细节见 [开发文档](docs/development.md) 与 [更新日志](CHANGELOG.md)。

## 仓库结构

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

## 许可证

[MIT](LICENSE)
