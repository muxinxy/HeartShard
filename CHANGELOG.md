# Changelog / 更新日志

All notable changes to HeartShard (牧心相册) are documented here.
HeartShard 的所有重要变更记录于此。

## [0.1.0] - 2026-11-12

首个公开版本。纯客户端、端到端加密的多云相册系统：照片/视频切片后以
AES-256-GCM 加密、Reed-Solomon 纠删码冗余，分散存储在用户自己的
S3 / WebDAV / OpenList / FTPS 存储上——服务器（可选）与云存储全程零知识。

First public release. A client-side, end-to-end encrypted multi-cloud photo
album: photos/videos are sharded, encrypted with AES-256-GCM, redundantly
erasure-coded (Reed-Solomon), and scattered across the user's own
S3 / WebDAV / OpenList / FTPS storage — zero-knowledge for both the optional
server and the cloud storage.

### Added / 新增

**核心 / Core**
- 加密分片管线：AES-256-GCM（96-bit tag）逐 chunk 加密，Leopard-RS 纠删码
  n=4/m=2 冗余分散，SHA-256 全程校验
  Encrypted sharding pipeline: per-chunk AES-256-GCM (96-bit tag),
  Leopard-RS erasure coding (n=4/m=2), SHA-256 verified throughout
- 主密钥封装：随机 MK + 口令派生 KEK 封装（改密码不重传分片）
  Master key wrapping: random MK wrapped by password-derived KEK
  (password change without re-uploading shards)
- 多设备增量同步：Lamport 时钟 + LWW 合并 + delta 发布
  Multi-device incremental sync: Lamport clocks + LWW merge + delta publish
- 云存储适配器：S3（手写 SigV4）、WebDAV、OpenList（令牌/账密）、FTPS
  Cloud storage adapters: S3 (hand-written SigV4), WebDAV, OpenList
  (token or credentials), FTPS

**后台服务 / Server**
- 牧心后台：多用户认证（argon2id）+ 相册级 ACL（none/view/upload/manage，
  沿树继承、显式覆盖、用户优先于组）+ 常驻同步引擎 + 数据 API
  Self-hosted server: multi-user auth (argon2id), album-level ACL
  (none/view/upload/manage with inheritance, override, user-over-group),
  resident sync engine, data API
- 元数据存储：SQLite（默认）或 MySQL，旧 JSON 配置自动迁移
  Metadata storage: SQLite (default) or MySQL, with automatic migration
  from legacy JSON config
- 网页端管理：用户/用户组/同步源/云存储全部网页端配置；存储连通性探针
  Web admin: users, groups, sync sources and cloud storage all configurable
  in the browser; storage connectivity probe
- 分享：相册或单张/多选照片分享链接，**默认去除 EXIF/GPS 元数据**，
  可选有效期与分享码；浏览器 WebCrypto 解密，服务器零知识
  Sharing: album or multi-select photo links with **EXIF/GPS metadata
  stripped by default**, optional expiry and access code; decrypted via
  WebCrypto in the browser — the server never sees plaintext
- PWA 网页客户端：时间线/探索（GPS 地点聚类 + 类型分类）/相册/收藏夹/
  回收站/多选批量操作/右键菜单/灯箱（缩放、旋转、翻转、视频播放）
  PWA web client: timeline, explore (GPS place clustering + type facets),
  albums, favorites, trash, multi-select batch actions, context menus,
  lightbox (zoom, rotate, flip, video playback)
- 动态照片：Android Motion Photo（JPEG 尾随 MP4）检测与视频段提取；
  Apple Live Photo 文件名配对；双端显示、播放与双文件导出
  Motion photos: Android Motion Photo (JPEG trailing MP4) detection and
  video extraction; Apple Live Photo filename pairing; display, playback
  and two-file export on desktop and web
- 部署：Dockerfile + docker-compose；GitHub Actions 多平台多架构编译
  （linux x86_64/aarch64/armv7/i686、windows x86_64、android
  arm64-v8a/armeabi-v7a/x86_64）
  Deployment: Dockerfile + docker-compose; GitHub Actions multi-platform,
  multi-architecture builds (linux x86_64/aarch64/armv7/i686,
  windows x86_64, android arm64-v8a/armeabi-v7a/x86_64)

**桌面客户端 / Desktop & Android**
- Windows 便携单文件 exe（数据目录 exe 同级）+ Android APK
  Windows portable single-file exe (data dir next to the exe) + Android APK
- 双模式：本地相册库（serverless 直连云存储）或连接牧心后台
  Dual mode: local library (serverless, direct cloud access) or connect to
  a self-hosted server
- 时间线（按天分组 + 年月导航）/ 瀑布流网格 / 相册封面墙 / 收藏夹 /
  回收站（可恢复）/ EXIF 解析 / 地图视图 / 加密视频在线播放（Range 流式）
  Timeline (day grouping + year/month rail), grid, album wall, favorites,
  restorable trash, EXIF parsing, map view, encrypted in-place video
  streaming (Range requests)
- 全链路调试日志（[http]/[auth]/[upload]/[meta] 等）
  End-to-end debug logging ([http]/[auth]/[upload]/[meta], etc.)

### Changed / 变更
- 全部用户可见提示中文化
  All user-facing messages localized to Chinese
- UI 遵循「相册优先、技术隐藏」：加密/分片细节折叠，交互对齐主流相册应用
  UI follows "album-first, tech-hidden": encryption/sharding details tucked
  away, interactions aligned with mainstream photo apps

### Known Limitations / 已知限制
- HEIC 静态图暂不解码（Live Photo 主图按原文件存取）；人脸/场景 AI 分类未实现
  HEIC still images are not decoded yet (Live Photo key images are stored
  as-is); no face/scene AI classification
- 视频分享暂不去元数据；`/raw` 视频整段载入内存
  Video shares keep metadata; `/raw` video loads fully into memory
- iOS 需 macOS/Xcode；鸿蒙壳需 DevEco（详见 docs/development.md）
  iOS requires macOS/Xcode; HarmonyOS shell requires DevEco (see
  docs/development.md)

[0.1.0]: https://github.com/muxinxy/HeartShard/releases/tag/v0.1.0
