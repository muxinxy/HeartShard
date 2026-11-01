# HeartShard 开发文档

[English](development.en.md)

> 面向本仓库贡献者的工程指南。产品定位与安全模型见 [README](../README.md)，算法研究见 [mixfile-study.md](mixfile-study.md)。

## 1. 环境搭建

### 1.1 工具链（mise 管理）

本项目用 [mise](https://mise.jdx.dev) 钉住工具链版本（见根目录 `mise.toml`）：rust stable（≥1.85，edition 2024）、node 24、pnpm。

```powershell
# 一次性环境准备（pwsh）
winget install jdx.mise          # 若未安装 mise
mise trust                       # 信任项目 mise.toml
mise install                     # 安装 rust/node/pnpm
rustup target add wasm32-unknown-unknown   # WASM 绑定用（M1 起）
```

验证：

```powershell
mise exec -- rustc --version   # ≥ 1.98
cargo test --manifest-path core-rs/Cargo.toml --workspace   # 38 个测试全绿
```

### 1.2 终端说明（重要）

- **贡献者终端**：本项目以 **pwsh** 为准。文档与 issue 中给出的命令均为 pwsh 语法（`$env:VAR="x"` 而非 `VAR=x cmd`）。
- **CI / 部分自动化工具内部**：在 Windows 上可能落到 cmd.exe（PATH 上有 BusyBox 工具）。写自动化脚本时避免依赖 `;` 作为命令分隔符（cmd 不支持）、避免 `for` 循环，改用 `&&` 链接或脚本文件。

### 1.3 Rust targets

| target | 用途 | 何时需要 |
|---|---|---|
| `x86_64-pc-windows-msvc` | 宿主开发/测试（默认） | M0 起 |
| `wasm32-unknown-unknown` | Tauri WebView WASM 绑定 | M1 起 |
| `aarch64-linux-android` 等 | Android（NDK） | M3 |
| `aarch64-apple-ios` | iOS | M5 |
| `aarch64-unknown-linux-ohos` | 鸿蒙 cdylib | M4（需 rust ≥1.78） |

## 2. 仓库布局与模块边界

```
core-rs/                     # Rust 核心 workspace
├── crates/keypath/          # 对象键生成 + sanitize_key_segment —— 四端唯一实现，壳层禁止自行实现
├── crates/crypto/           # bare 层（MixFile 对齐）/ KDF / AAD chunk 层
├── crates/ida/              # Reed-Solomon 分散（Leopard-RS 封装）
├── crates/shard/            # blob→chunk→加密→分散→重组管线
└── bindings/                # wasm/（Tauri）与 ffi/（鸿蒙 napi）—— M1/M4 加入
apps/tauri/                  # Windows + Android + iOS 共用 UI 壳（M1 起）
apps/harmony/                # 鸿蒙 ArkTS 壳（M4 起）
tools/keypath-tests/         # 跨端键路径契约：vectors.json + Rust 校验器
docs/fixtures/               # 机器可读测试向量 + 生成脚本（Java/Python 双来源）
.research/                   # 上游参考仓库（gitignore，勿提交）
```

硬边界（不可协商）：

1. `sanitize_key_segment` 与对象键生成器**只**存在于 `core-rs/crates/keypath`；新端必须消费共享向量（`tools/keypath-tests/vectors.json`）证明一致。
2. 云存储上永远只有哈希对象键——时间戳、顺序 ID、原始文件名禁止进入任何对象键。相册名等用户可控字符串只存在于加密索引。
3. 日志不打印分片内容、完整 URL query、密钥或口令。

## 3. 构建与测试

```powershell
cargo test --manifest-path core-rs/Cargo.toml --workspace     # 全部单元/向量测试
cargo run --manifest-path tools/keypath-tests/Cargo.toml      # 键路径共享向量（40 条）
cargo clippy --manifest-path core-rs/Cargo.toml --workspace --all-targets   # 0 警告基线
```

### 3.1 测试向量 provenance 链

向量不是拍脑袋写的，双独立实现交叉验证后由 Rust 锁定：

```
Java VectorGen.java（与 mixfile-core 相同 JCE 调用）
        │  生成 mixfile-vectors.json（V1–V4）
        ▼
Python crosscheck.py（OpenSSL 后端，逐字节比对）── 全 PASS
        │
        ▼
Rust core-rs/crates/crypto/tests/vectors.rs（逐字节断言）

Python gen-heartshard-vectors.py（cryptography + argon2-cffi）
        │  生成 heartshard-vectors.json（K1/H1/H2：Argon2id、HKDF、AAD 密文）
        ▼
Rust 同上断言
```

修改 crypto 层前先跑向量测试；向量文件本身视为契约，变更需同步三端脚本并说明理由。

## 4. 核心设计决策速查

细节与源码证据见 [mixfile-study.md](mixfile-study.md)；此处只列结论：

| 决策 | 结论 |
|---|---|
| 分片加密 | AES-256-GCM，12B 随机 IV，96-bit tag，输出 `iv‖ct‖tag` |
| 密钥链 | 口令 → Argon2id(m=64MiB, t=3, p=4；低配 32MiB) → MK → HKDF：`manifest_key` / `config_key` / `blob_key` |
| blob_key（裁决 Q2） | `HKDF(MK, salt=master_salt, info="heartshard/blob/v1"‖blob_hash)`；每 chunk 随机 IV，杜绝 GCM nonce 重用 |
| AAD | `blob_hash(32B) ‖ chunk_index(u32 BE) ‖ total_chunks(u32 BE)` |
| MixFile 兼容（裁决 Q3） | 不兼容：不解析 `mf://` 分享码，不读图床内容 |
| 纠删码（裁决 Q6） | n = 总 shard 数，m = 恢复阈值（默认 4/2，存储放大 2×）；Leopard-RS，manifest 记录 codec 标识 |
| 对象键 | `blobs/{h[0:2]}/{h[2:4]}/{hash}/{chunk:05}_{shard:02}.shard`；manifest 同构；两级十六进制桶共 65,536 |
| 分片大小 | 256 KiB–4 MiB（默认 1 MiB），全局 < Storage < 上传会话三层覆盖，写入 manifest 后不可变 |
| 单目录对象数 | 上限 5,000（FTP 建议 2,000），接近上限只告警不自动迁移 |
| 相册树 | 只存在于加密索引；目录导入深度默认 8 层截断；`import_source` 为 local_only |

## 5. 里程碑计划（平台优先级 2026-11-01 调整）

**Windows 与 Android 优先**：Android 从原 M3 的"Android/iOS 并行"拆出提前，iOS 延后至 M5。

| 里程碑 | 交付物 | 退出标准 |
|---|---|---|
| **M1** | Tauri Windows：上传 → 本地目录模拟 Storage → 下载还原 → SHA256 校验 | 端到端可运行，校验通过 |
| **M2** | S3 适配器 + 索引同步双设备模拟 | 双设备时间线一致；Lamport 冲突裁决正确 |
| **M3** | **Android** Tauri 壳 + 后台传输（Foreground Service）+ WebDAV 适配器 | 移动端上传不被系统回收；WebDAV 大文件分块续传 |
| **M4** | 鸿蒙 ArkTS 壳 + `ohos_napi_rs` + OpenList 适配器 | 鸿蒙可上传/下载/浏览；panic 不穿 FFI |
| **M5** | iOS 壳 + FTP/FTPS + 修复/再平衡 + 威胁模型文档 | 四端功能对齐；修复全链路 E2E |

每个里程碑结束输出：可运行命令、已知平台限制、下一阶段前置依赖、截图或 curl 示例。

## 6. 提交纪律

1. **先验证外部 API，再写适配器**——用最小脚本（curl/PowerShell）跑通真实服务的字段与行为，不凭记忆写死。
2. 按里程碑提交，每个 commit 保持可运行（测试全绿）。
3. 参数歧义以源码/实测为准，记录证据到 `docs/`，不猜测。
4. 提交前：`cargo test --workspace` + 前端 `pnpm test` / `pnpm typecheck`（M1 起）。
5. 鸿蒙 FFI 边界：所有暴露给 ArkTS 的 Rust 函数必须 `catch_unwind` + 错误码返回，panic 不许穿过。

## 7. 已知限制与遗留（随里程碑消化）

- RS 基准数据来自 crate 官方，M1 需在本机 release 模式实测（1 MiB chunk、n=4）。
- `DerivedKeys` 持有 MK（会话内存），`Hkdf` prk 未 Drop 零化 → M2 加固。
- shard 测试重复构造 Argon2 密钥（debug ~2-3s/次）→ M1 引入会话级密钥缓存。
- gzip 字节级跨语言一致不列入契约（以解压内容为准）；实测 flate2 与 Java 17 头字节一致（OS=0xff）。
