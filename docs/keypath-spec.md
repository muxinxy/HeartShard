# HeartShard 对象键规范（keypath-spec）

> 单一实现：`core-rs/crates/keypath`。四端壳层**禁止**自行实现或复制本文件逻辑；
> 一致性契约：`tools/keypath-tests/vectors.json`（40 条共享向量 + Rust 校验器）。

## 1. 对象键格式

| 对象 | 格式 | 示例 |
|---|---|---|
| shard | `blobs/{hash[0:2]}/{hash[2:4]}/{blobHash}/{chunkIndex:05d}_{shardIndex:02d}.shard` | `blobs/a3/f1/a3f1…8f90/00042_03.shard` |
| manifest | `manifests/{hash[0:2]}/{hash[2:4]}/{blobHash}/manifest.bin` | `manifests/a3/f1/a3f1…8f90/manifest.bin` |
| 远端索引 | `index/{deviceId}/{lamportClock}.idx` | `index/device01ab/42.idx` |

- `blobHash`：64 位小写十六进制 SHA-256。
- 两级十六进制桶共 65,536 个目录，为个人相册规模提供充分打散。
- **禁止**时间戳、顺序 ID、原始文件名或任何用户可控字符串进入对象键。

## 2. `sanitize_key_segment` 校验规则

按顺序执行，任一失败即拒绝：

1. 非空，且 ≤ 255 字节（UTF-8）；
2. 仅允许小写 ASCII 字母、数字、`_`、`-`、`.`；
3. 不得以 `.` 开头或结尾（排除 `.`/`..` 与 Windows 尾点截断）；
4. 不得包含 Windows 保留名**子串**（不区分大小写）：
   `CON, PRN, AUX, NUL, COM1–COM9, LPT1–LPT9`。

说明：子串规则比 Windows 实际语义（仅 basename 匹配）更严格——这是项目的
确定性约定（简报 §5.2「及其子串」）。生成的键只含十六进制与固定字面量，
永不触发；此规则防御未来可能的手工键。

## 3. 单目录对象数上限

- 默认 **5,000**（兼容 PureFTPD 10,000 列举上限与 WebDAV PROPFIND 性能）；
- FTP 类型 Storage 建议 **2,000**；
- 接近上限 → 同步/修复任务输出警告，**不自动迁移**（简报 §5.4）。

## 4. 跨端一致性

其他端实现（ArkTS/JS、未来任何壳层）以 `vectors.json` 为契约：
逐条跑 sanitize 与键生成，输出必须与 Rust 校验器一致：

```powershell
cargo run --manifest-path tools/keypath-tests/Cargo.toml   # Rust 侧 40 条应全 PASS
```

## 5. 生成器 API（Rust）

```rust
keypath::shard_object_key(&blob_hash, chunk_index: u32, shard_index: u8) -> Result<String>
keypath::manifest_object_key(&blob_hash) -> Result<String>
keypath::index_object_key(device_id: &str, lamport: u64) -> Result<String>
keypath::sanitize_key_segment(&str) -> Result<&str, KeypathError>
keypath::validate_blob_hash(&str) -> Result<&str, KeypathError>
```
