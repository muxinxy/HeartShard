# HeartShard · 牧心相册

> **把记忆碎成片，散养在你信任的每一朵云上。**
>
> Shatter your memories into pieces — and let every cloud you trust raise them.

[English](README.en.md)

HeartShard 是一个**纯客户端、无后端、端到端加密**的多云相册系统，覆盖 Windows / Android / iOS / 鸿蒙四端。照片和视频被切碎、加密、纠删冗余后分散到用户自己的云存储（S3 兼容 / WebDAV / OpenList / FTP）。**没有服务器、没有账号体系、没有中心化索引**——云存储只作为被动 Blob 仓库，所有解密、索引、同步逻辑都运行在客户端。

面向用户：想把照片存在多个云盘 / NAS / S3 桶中，但不愿信任任何单一服务商的隐私敏感用户。

## 核心特性

- 🔐 **端到端加密**：AES-256-GCM 分片加密，每 blob 独立密钥（HKDF 派生），每 chunk 随机 IV；口令经 Argon2id 派生主密钥，口令与密钥绝不落盘
- 🧩 **分片 + 纠删码**：文件按 256 KiB–4 MiB 切块，Reed-Solomon（默认 n=4, m=2）分散冗余，任一云存储整体丢失仍可完整恢复
- 🗂 **目录即相册**：Windows 端目录导入为加密索引中的相册树，云上永远看不到原始文件名与目录结构
- 🔄 **多设备同步**：纯客户端 Lamport 时钟合并，无实时强一致依赖
- ☁️ **多云分散**：同一 blob 的多个 shard 强制落在不同 Storage；支持 S3 / WebDAV / OpenList / FTP，权重与健康检查
- 📱 **四端一套核心**：核心逻辑只有一份（Rust），Tauri（Windows/Android/iOS）与 ArkTS 原生（鸿蒙）两套壳共享

## 当前进度

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | MixFile 算法研究 + core-rs（keypath / crypto / ida / shard）+ 测试向量锁定 | ✅ 完成（`fe5d3e6`） |
| **M1** | Tauri Windows 端：上传 → 本地目录模拟 Storage → 下载还原 → SHA256 校验 | ✅ 完成（E2E 全绿） |
| **M2** | S3 适配器（SigV4）+ SQLCipher 索引 + 双设备 Lamport 同步 | ✅ 完成（双设备 E2E 时间线一致，冲突裁决单测覆盖） |
| **M3** | **Android** Tauri 壳（aarch64 APK）+ WebDAV 适配器 + 后台传输 | ✅ 完成（壳/WebDAV/前台服务防回收；§8.5 五阶段向导 UI + import_staging 续传已实现） |
| **M4** | 鸿蒙 ArkTS 壳 + Rust FFI + OpenList 适配器 | OpenList 适配器 ✅（mock 全端点测试，真实实例验收待用户服务）；鸿蒙壳 ⚠️ 需 DevEco/OHOS SDK（本开发机不可用），见 [开发文档](docs/development.md) |
| **M5** | iOS + FTP/FTPS + 修复/再平衡任务 + 威胁模型文档 | 修复 ✅、再平衡 ✅、FTP/FTPS ✅（真实服务验收待端点）、[威胁模型](docs/threat-model.md) ✅；iOS ❌ 需 macOS |
| **§10 UI** | 时间线虚拟滚动（Virtuoso）+ 懒加载缩略图（加密缓存）+ 详情页（分片分布可视化）+ 一键体检 + 任务中心 + 历史报告重开 | ✅ 完成 |
| **§11** | 缩略图本地生成并加密缓存 | ✅ 完成（manifest_key 加密，`thumbs/{hash}.enc`） |
| **产物** | Windows 便携 exe + 安装包；Android APK（aarch64） | ✅ `dist/` 下四件（见上）+ §12.2 三场景 E2E 全绿 |

> 平台优先级（2026-11-01 调整）：**Windows → Android 优先**，iOS 与鸿蒙随后。
> 剩余如实声明的缺口：地图视图（§10）、真 EXIF 解析（当前用文件时间，M1.5）、
> 「鸿蒙 100MB WebDAV 分块」E2E（随鸿蒙壳）、增量同步 delta、各协议真实端点验收。

## 快速开始（开发）

环境要求：[mise](https://mise.jdx.dev) 管理工具链，终端推荐 pwsh。

```powershell
git clone <repo-url> heartshard; cd heartshard
mise install          # rust stable + node 24 + pnpm
cargo test --manifest-path core-rs/Cargo.toml --workspace   # 38 个测试应全绿
cargo run --manifest-path tools/keypath-tests/Cargo.toml    # 键路径共享向量校验（40 条）
```

更多开发细节（环境搭建、架构约定、测试向量链路、提交纪律）见 [开发文档](docs/development.md)。

## 仓库结构

```
heartshard/
├── core-rs/                    # Rust 核心，四端共享（编译为 WASM / cdylib）
│   └── crates/
│       ├── keypath/            # 对象键生成 + 文件名校验（四端唯一实现）
│       ├── crypto/             # AES-256-GCM 分片加密 + Argon2id/HKDF 密钥派生
│       ├── ida/                # Reed-Solomon 纠删码分散
│       └── shard/              # blob → chunk → 加密 → 分散 → 重组全管线
├── apps/                       # 平台壳（M1 起）
├── tools/keypath-tests/        # 跨端键路径一致性测试（共享向量 + 校验器）
├── docs/                       # 研究与开发文档
│   ├── mixfile-study.md        # MixFile 算法研究报告（含测试向量与设计裁决）
│   ├── fixtures/               # 机器可读测试向量 + 生成脚本（Java/Python 双来源）
│   └── development.md          # 开发文档
└── .research/                  # 上游参考仓库克隆（gitignore）
```

## 安全模型（摘要）

- 云存储服务商只能看到：加密分片、不可读的哈希对象键、加密的 manifest 与索引
- 口令不落盘，主密钥只存在于会话内存；存储配置中的敏感字段用派生密钥加密后落库
- 日志脱敏：不打印分片内容、完整 URL query、任何密钥
- 完整性链条：per-shard SHA-256 → RS 重组 → GCM tag/AAD（绑定 blob/分片位置）→ per-chunk 与 blob 级 SHA-256

完整威胁模型文档随 M5 交付；算法对齐与设计裁决见 [docs/mixfile-study.md](docs/mixfile-study.md)。

## 许可证

待定（TBD）。
