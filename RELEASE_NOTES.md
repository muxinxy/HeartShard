# HeartShard v0.1.0 · 牧心相册 首个公开版本

**HeartShard (牧心相册)** —— 纯客户端、端到端加密的多云相册系统。照片/视频切片后以 AES-256-GCM 加密、Reed-Solomon 纠删码冗余，分散存储在**你自己**的 S3 / WebDAV / OpenList / FTPS 上：云存储与（可选的）自托管后台全程零知识。

**HeartShard** — a client-side, end-to-end encrypted multi-cloud photo album. Photos/videos are sharded, encrypted with AES-256-GCM, redundantly erasure-coded (Reed-Solomon), and scattered across **your own** S3 / WebDAV / OpenList / FTPS storage — zero-knowledge for the cloud and the (optional) self-hosted server alike.

完整变更清单见 [CHANGELOG.md](CHANGELOG.md) · Full changelog: [CHANGELOG.md](CHANGELOG.md)

---

## 下载 / Downloads

> CI 构建产物在本次推送的 [Actions 运行](https://github.com/muxinxy/HeartShard/actions/workflows/build.yml) 中生成并上传为 artifacts；以下为本地验证构建。

> CI artifacts are produced by the [Actions run](https://github.com/muxinxy/HeartShard/actions/workflows/build.yml) triggered by this release; the following are locally-verified builds.

| 文件 File | 平台 Platform | 说明 Notes |
|---|---|---|
| `HeartShard-0.1.0-x64-portable.exe` | Windows x64 | 便携单文件，数据目录为 exe 同级 `data/`；Portable single-file exe, data dir next to the exe |
| `HeartShard-0.1.0-arm64.apk` | Android arm64-v8a | 直接安装；Install directly |
| `heartshard-server-*`（CI 产物） | linux (x86_64/aarch64/armv7/i686) | 后台服务；Self-hosted server |

SHA256（本地构建 / local builds）：

```
79538AC92346…  HeartShard-0.1.0-x64-portable.exe
ED004BB092BB…  HeartShard-0.1.0-arm64.apk
```

## 快速开始 / Quick Start

### 桌面 + 安卓（纯 serverless，不需要服务器）/ Desktop + Android (pure serverless)

1. 启动桌面客户端 → 「存储」→ 添加你的 S3/WebDAV/OpenList/FTPS 存储
   Launch the desktop app → “存储” → add your S3/WebDAV/OpenList/FTPS storage
2. 「导入照片 / 视频」或拖拽导入——照片自动切片、加密、冗余分散
   Import or drag & drop — photos are auto-sharded, encrypted and scattered
3. 安卓端用**同一个数据目录的 mk.wrap**（多设备共享主密钥）即可看到全部照片
   On Android, share the same `mk.wrap` from the desktop data dir to see all photos

### 自托管后台（多用户 / 网页端 / 分享链接）/ Self-hosted server

```bash
docker compose up -d        # 见仓库 docker-compose.yml，口令写 .env
# 或直接运行二进制（口令终端交互输入）：
./heartshard-server --config server.toml
```

浏览器打开 `http://NAS:8420`：多用户、相册权限、同步源、存储、分享链接全部网页端管理。手机 PWA「添加到主屏幕」即免上架使用（需 HTTPS）。

Open `http://NAS:8420` in a browser: multi-user accounts, album permissions, sync sources, storage and share links are all managed on the web. On phones, “Add to Home Screen” installs the PWA (HTTPS required).

## 已知限制 / Known limitations

- HEIC 静态图暂不解码（Live Photo 主图按原文件存取）
  HEIC stills are not decoded yet (Live Photo key images stored as-is)
- 视频分享暂不去元数据；人脸/场景 AI 分类未实现
  Video shares keep metadata; no face/scene AI yet

---

完整文档：[docs/development.md](docs/development.md) · README：[README.md](README.md)
