# HeartShard 存储适配器（storage-adapters）

> 云存储是**被动 Blob 仓库**：只需四个原语 `put / get / list / delete`。
> Trait 定义在 `core-rs/crates/index/src/store.rs`（`BlobStore`），
> 实现位于 `apps/tauri/src-tauri/src/storage_*.rs`。

## 1. Trait

```rust
pub trait BlobStore: Send + Sync {
    fn id(&self) -> &str;                                  // 诊断/报告用标识
    fn put(&self, remote_key: &str, data: &[u8]) -> Result<(), StoreError>;
    fn get(&self, remote_key: &str) -> Option<Vec<u8>>;    // None = 不存在
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
    fn delete(&self, remote_key: &str) -> Result<(), StoreError>; // 幂等
}
```

## 2. 实现矩阵（简报 §9.3 顺序）

| 适配器 | 文件 | 状态 | 验证方式 |
|---|---|---|---|
| 本地目录（模拟） | `index::store::LocalDirStore` | ✅ 全测试覆盖 | crate 单测 + E2E |
| S3 兼容 | `storage_s3.rs`（手写 SigV4 + reqwest/rustls） | ✅ 代码完成 | AWS 官方签名向量 + mock server 集成测试 + `s3-verify` bin（真实端点） |
| WebDAV | `storage_webdav.rs`（PUT/GET/DELETE/PROPFIND/MKCOL） | ✅ 代码完成 | mock WebDAV 服务器集成测试 |
| FTP/FTPS | `storage_ftp.rs`（suppaftp 12, rustls-ring） | ✅ 代码完成 | 纯逻辑单测；**真实 FTPS 服务连通性验证待用户提供端点后执行** |
| OpenList | 复用 S3 适配器（OpenList 提供 S3 网关）或 WebDAV | 计划 | 先 curl 验证 `/api/fs/put` 认证行为（token 不加 Bearer 前缀，以实测为准） |

## 3. 验证纪律（简报 §14）

**先验证外部 API，再写适配器**：

```powershell
# S3 真实端点验收（用户提供凭据后）
$env:S3_ENDPOINT="https://…"; $env:S3_BUCKET="…"
$env:S3_ACCESS_KEY="…"; $env:S3_SECRET_KEY="…"
$env:S3_REGION="us-east-1"; $env:S3_PATH_STYLE="1"
cargo run -p heartshard-tauri --bin s3-verify
```

WebDAV/FTP 真实端点验收脚本同构（复用同一 BlobStore 面板）。

## 4. 平台注意事项（简报 §9）

- **Windows**：适配器在 Rust 侧直连（reqwest + rustls，不依赖 schannel）。
- **Android/iOS**：后台传输需 Foreground Service / URLSession（Tauri plugin，M3 路线）；
  适配器本体复用。
- **鸿蒙**：WebDAV 大文件受 `@ohos.net.http` 内存 `ArrayBuffer` 限制，
  ArkTS 层必须分块 `Content-Range` 续传（平台层实现，Rust 适配器不适用该约束）。

## 5. Shard 放置策略

- shard 按 `shard_index % len(active_storages)` 轮转放置；
- 同一 chunk 的多个 shard 必落**不同** Storage（Storage 数 ≥ n 时天然满足；
  不足时轮转不重复，§2.2）；
- manifest 加密后**冗余写入全部** Storage（≥2，§6.4）；
- 读取 manifest：任一 Storage 命中即可。

## 6. 已知限制

- 单目录对象数 5,000 上限只告警不迁移（M5 运维任务）。
- FTP 适配器缺真实服务验证（见上表）。
- S3 `list` 分页已实现（continuation-token），但未在 mock 中覆盖 >1000 对象场景。
