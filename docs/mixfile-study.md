# MixFile 算法研究报告（HeartShard M0 前置）

> 状态：**初稿，待确认**。本文档确认之前不开始 `core-rs/crates/crypto` 编码。
>
> 研究日期：2026-11-01。所有结论均以 `mixfile-core` 源码为准（文档站仅作旁证）。

## 0. 研究来源与版本锁定

| 仓库 | 用途 | 本地克隆 | HEAD |
|---|---|---|---|
| [InvertGeek/mixfile-core](https://github.com/InvertGeek/mixfile-core) | 核心加密/上传/下载逻辑（**结论以此为准**） | `.research/mixfile-core` | `10a2953` |
| [InvertGeek/MixFile](https://github.com/InvertGeek/MixFile) | Android 主仓库（配置默认值、UI 侧调用） | `.research/MixFile` | `580f60d` |
| [InvertGeek/mixfilecli](https://github.com/InvertGeek/mixfilecli) | CLI（Ktor web 上传/下载，配置结构旁证） | `.research/mixfilecli` | `39582c1` |
| [InvertGeek/mixfile-go](https://github.com/InvertGeek/mixfile-go) | 第三方 Go 移植（参数交叉印证） | `.research/mixfile-go` | `5531b92` |
| [官方文档站](https://invertgeek.github.io/mixfile-doc/) | 索引容量、分片大小的官方表述 | — | — |

源码行号引用格式 `文件:行号`，均指上述 HEAD 版本。

---

## 1. 参数表（源码证据）

### 1.1 AES-GCM 核心

| 参数 | 值 | 源码证据 |
|---|---|---|
| 算法 | `AES/GCM/NoPadding` | `mixfile-core/.../aes/AES.kt:40` |
| 密钥长度 | **32 字节（AES-256）** | `routes/api/routes/UploadRoute.kt:49`：`key: ByteArray = generateRandomByteArray(32)`；Go 移植逐行对应 |
| IV/Nonce | **12 字节，每次加密随机生成**（SecureRandom） | `AES.kt:23`：`iv: ByteArray = generateRandomByteArray(12)` |
| GCM tag 长度 | **96 bit（12 字节）**，非默认 128 bit | `AES.kt:42`：`GCMParameterSpec(96, iv)`；Go 移植 `aes/aes.go:10`：`const tagSize = 12 // 严格对应 Kotlin 的 GCMParameterSpec(96, iv)` |
| AAD | **不使用** | `AES.kt:43`：`cipher.init(mode, secretKey, gcmParameterSpec)` 无 AAD 参数；Go 移植 `aes.go:44`：`Seal(iv, iv, data, nil)` |
| 输出布局 | **`iv(12B) ‖ ciphertext ‖ tag(12B)`** | `AES.kt:26`：`return iv + cipher.doFinal(data)`（JCE `doFinal` 返回 `ct‖tag`） |
| 解密 | 从数据头取 12B 作 IV，其余为 `ct‖tag` | `AES.kt:29-37`；流式版 `AES.kt:48-70`（limit+12 字节上限防超大分片） |

**结论：AES-256-GCM，随机 12B IV 前置，96-bit tag，无 AAD。**
文档站与 README 声称的"AES-256"（`MixFile/README.MD` 安全性章节）与源码一致；
不存在 128 位主数据加密路径——唯一的 16 字节密钥出现在分享码层（见 1.4），
这很可能就是"AES-128 说法"的来源（详见 §5 未决问题 Q1）。

GCM tag 截断说明：GCM 规范（NIST SP 800-38D）允许 tag 取完整 128 位输出前 t 位，
因此 12B tag = 16B tag 的前 12 字节。Rust `aes-gcm` crate 用 `AesGcm<Aes256, U12>` 直接支持，无需手工截断。

### 1.2 分片（chunk）

| 参数 | 值 | 源码证据 |
|---|---|---|
| 默认分片大小 | **1 MiB**（`1024 KB`） | `MixFileServer.kt:38`：`open val chunkSize = 1.mb`；Android `FileService.kt:47`：`MIXFILE_CHUNK_SIZE by cachedMutableOf(1024, "mixfile_server_chunk_size")`，`:72`：`chunkSize = MIXFILE_CHUNK_SIZE.toInt() * 1.kb` |
| 可配置 | **是**（`open val`，Android 按 KB 设置，CLI `config.yml` 的 `chunk_size: 1024`） | `MixFileServer.kt:38`、`mixfilecli/Application.kt` Config |
| 硬上限 | **20 MiB** | `UploadRoute.kt:100`：`val fixedChunkSize = min(20.mb, chunkSize)` |
| 写入索引 | chunk 实际值写入 `MixFile.chunk_size`（`chunk_size` 字段），下载按此切 Range | `MixFile.kt:13`、`UploadRoute.kt:147`、`DownloadRoute.kt:128` |
| 分片方式 | 精确读满 chunkSize（末片除外），**无任何填充** | `UploadRoute.kt:173-186`（`readChunkExact`，注释解释了为何不能用 `readRemaining`） |
| 索引容量关系 | 官方：**1 MB 索引 ≈ 1.5w–2w 条分片信息 ≈ 15–20 GB 文件**；分片翻倍，索引容量翻倍 | help.html《分片大小》章节 |

### 1.3 SHA-256 校验层级

| 参数 | 值 | 源码证据 |
|---|---|---|
| 校验粒度 | **每个 chunk 独立校验**（README：「每个文件分片均使用 SHA256 哈希校验」） | `Uploader.kt:63` |
| 校验对象 | **chunk 明文**（加密前数据），非密文 | `Uploader.kt:63`：`fragment = fileData.hashMixSHA256()`，`fileData` 是加密前入参 |
| 编码方式 | `base62(SHA-256(明文))`，即 `hashMixSHA256()` | `MixShareInfo.kt:9`：`MixShareInfo.ENCODER.encode(hashSHA256())` |
| 存放位置 | **chunk 下载 URL 的 `#fragment` 中**（索引 `file_list` 的每条 URL 自带校验和） | `Uploader.kt:62-64`、`MixShareInfo.kt:111`：`val hash = Url(url).fragment.trim()` |
| 校验时机 | **仅下载时**（上传后无服务器侧校验概念，图床无 etag 可查） | `MixShareInfo.kt:113-118`：`if (!currentHash.contentEquals(hash)) throw NoRetryException("文件遭到篡改")` |
| 索引文件自身 | 索引（gzip 后的 `MixFile` JSON）用同一 `uploader.upload()` 上传，**其 URL fragment 同样携带索引明文的 base62 SHA-256**，`fetchMixFile` 时校验 | `UploadRoute.kt:157`、`MixShareInfo.kt:147-150` |
| blob 级校验 | **无**（MixFile 没有 blob 级 SHA-256；`MixShareInfo.s` 只有文件大小） | `MixShareInfo.kt:20-27` |

**结论：MixFile 的校验 = 每 chunk 明文 SHA-256（base62），随 URL fragment 携带，下载时逐片验证，无 blob 级、无上传后验证。**

### 1.4 索引 / 分享码结构

**索引对象（`MixFile`，相当于 HeartShard 的 manifest 前身）：**

```json
{"chunk_size":1048576,"file_size":1572864,"version":0,"file_list":["https://host/x#c0hash","..."]}
```

- 字段：`chunk_size`（Int）、`file_size`（Long）、`version`（Long，**恒为 0**，`UploadRoute.kt:148`，无实际作用）、`file_list`（chunk URL 数组，URL 自带 hash fragment）。
- 序列化链：JSON（kotlinx.serialization，字段按声明序，无空格）→ **gzip**（`Util.kt:136-142`，标准 `GZIPOutputStream`）→ `encryptAES`（同 key 同格式）→ 作为"一个文件"上传到图床。
- Range/seek：按 `chunk_size` 整除定位起始 chunk（`MixFile.kt:24-32`），支持视频拖动（`DownloadRoute.kt:87-95`）。

**分享信息（`MixShareInfo`，随分享码携带）：**

| JSON 键 | 字段 | 说明 |
|---|---|---|
| `f` | fileName | 原始文件名（**明文进入分享码**） |
| `s` | fileSize | 原始文件大小 |
| `h` | headSize | **隐写载体图片头字节数**，下载时丢弃前 `h` 字节（`MixShareInfo.kt:106`：`channel.discard(headSize)`） |
| `u` | url | 索引文件 URL（fragment 带索引 hash） |
| `k` | key | **AES 密钥本体，base62 编码，随分享码明文携带** |
| `r` | referer | 图床防盗链 referer |

- 分享码编码：`MixShareInfo` JSON → `encryptAES(bytes, DEFAULT_PASSWORD)` → base62。
- `DEFAULT_PASSWORD = "123".hashMD5()`（`MixShareInfo.kt:37`）= **MD5("123") = `202cb962ac59075b964b07152d234b70`（16 字节 → AES-128）**，硬编码。分享码加密只是防随手可读的混淆，**不提供保密性**（密码公开在源码里）。
- 短分享码：`mf://` 前缀 + hex 每字符用 Unicode 变体选择符（U+FE00–U+FE0F）隐藏（`ShareCode.kt:50-68`）。HeartShard 不使用。
- base62 字母表：`0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ`，前导 `0x00` 字节编码为前导 `'0'` 字符（`MixShareInfo.kt:34-35`、`basen/BigIntBaseN.kt`；Go 移植 `basen.go` 用 `big.Int.Text(62)` 逐位对应）。

**密钥封装方式结论：MixFile 没有真正的密钥封装——随机 32B 密钥 base62 后放进分享码，分享码用硬编码密码做混淆加密。密钥与索引地址不分家，拿到分享码 = 拿到一切。**

### 1.5 隐写层（HeartShard 不采用，但需理解）

- 每次上传先生成/抓取一张**载体图片**（`uploader.genHead()`；core 默认 1 像素 GIF，`MixFileServer.kt:53-54`；Android 端随机空白 GIF，`FileService.kt:104-106`；CLI 随机尺寸 GIF）。
- 实际上传体 = `head图片字节 ‖ encryptAES(数据, key)` = `图片 ‖ iv(12) ‖ ct ‖ tag(12)`（`Uploader.kt:73-75`）。
- `headSize`（MixShareInfo.h）记录图片长度；下载时 `discard(headSize)` 再解密。**图片部分不参与加密也不参与 hash**。
- "隐写"实为**前缀追加**而非像素级隐写：图床只看到一张尾端带高熵数据的图片。
- 上传返回 URL 后，把 `#base62(明文SHA256)` 追加到 URL（`Uploader.kt:62-64`）。

### 1.6 其他参数

| 参数 | 值 | 源码证据 |
|---|---|---|
| 上传并发 | 10（按 chunkMB 动态折减：`uploadTaskCount / max(chunkSizeMB, 1)`） | `MixFileServer.kt:28`、`UploadRoute.kt:90-92` |
| 下载并发 | 5，同样按 chunkMB 折减 | `MixFileServer.kt:26`、`DownloadRoute.kt:128-129` |
| 上传重试 | 10 次（delay 100ms） | `MixFileServer.kt:30`、`Uploader.kt:51` |
| 下载重试 | 3 次 | `MixFileServer.kt:32`、`MixShareInfo.kt:129` |
| 单分片下载上限 | 20 MB（`limit = 20.mb`，超限抛"分片文件过大"） | `MixShareInfo.kt:88,101-103` |
| 索引解压上限 | 200 MB | `Util.kt:144` |
| `getRandomEncKey()` | 返回 256 字节随机数，**全仓库无调用点（死代码）**，勿混淆 | `Util.kt:134` |

---

## 2. HeartShard 与 MixFile 的差异（设计决策）

### 2.1 保留 / 对齐的部分

| 项 | 决策 |
|---|---|
| 分片加密原语 | **完全对齐**：AES-256-GCM、12B IV、96-bit tag、无 AAD 的"裸 `encryptAES`"作为基础层实现，用 §4 测试向量逐字节锁定（Rust `aes-gcm` crate `AesGcm<Aes256, U12>`） |
| 分片模型 | 文件 → 固定大小 chunk（末片可短）→ 逐片独立加密。chunk_size 写入 manifest，已上传 blob 不可变 |
| chunk 校验 | 保留"每 chunk 明文 SHA-256"，但存放位置从 URL fragment 改为 manifest 字段（见 2.2） |
| base62 | 保留 MixFile 字母表与前导零语义（分享码兼容与测试向量需要） |
| 分片大小 | 默认 1 MiB、可配置。HeartShard 收紧为 256 KiB–4 MiB（MixFile 上限 20 MiB，无下限） |
| 索引容量公式 | 沿用：1 MB 索引 ≈ 1.5w–2w 条分片记录；HeartShard manifest 记录字段更多（见 2.2），按 1 MB ≈ 8k–1.2w 条预估 |

### 2.2 变更的部分（及理由）

| # | MixFile | HeartShard | 理由 |
|---|---|---|---|
| 1 | 分片藏进图片（`head ‖ iv‖ct‖tag`）上传图床 | **直接存储 `iv‖ct‖tag`**，无 head、无图床 | 对象存储不需要伪装；去掉隐写层 = 去掉 `genHead`/`headSize` 整条链路。编解码层抽象为 `encrypt_chunk(key, iv, pt) -> iv‖ct‖tag`，与存储完全解耦 |
| 2 | 存储 = 图床 URL（URL 同时承担定位+校验） | 存储 = 用户自有 S3/WebDAV/OpenList/FTP，对象键按 `keypath-spec`（SHA-256 打散） | 核心定位差异；URL fragment 不存在，校验和移入 manifest |
| 3 | 密钥随机生成、base62 明文放分享码 | 口令 → Argon2id(m=64MiB, t=3, p=4) → MK(32B) → HKDF 派生 `ENC_KEY`/`MANIFEST_KEY`/`CONFIG_KEY`/`IDA_SEED`；**密钥永不随 manifest 携带** | MixFile 的封装是混淆不是安全；HeartShard 威胁模型要求云上看不到密钥，且单设备失窃不泄露相册 |
| 4 | 无纠删码，靠同一文件传多个图床 | 加密后 chunk 做 **RS(n=4, m=2)** 纠删码，shard 分散到不同 Storage | 自有存储可能整桶丢失/损坏；RS 是对 MixFile"多副本冗余"意图的扩展而非照搬 |
| 5 | chunk 明文 SHA-256 在 URL fragment | manifest 记录：chunk 明文 SHA-256（复用 MixFile 语义）+ **每个 shard 密文 SHA-256** + blob 级 SHA-256 | RS 重建前必须剔除损坏 shard → 需要 shard 级校验；下载后 blob 级校验是 HeartShard 的完整性底线 |
| 6 | 索引 = `{chunk_size, file_size, version, file_list[]}` | manifest = `{version, chunk_size, n, m, blob_sha256, chunks[{pt_sha256}], shards[{storage_id, remote_key, size, etag, sha256}]}`，AES-GCM 加密后冗余存 ≥2 个 Storage | 需要承载冗余拓扑与修复信息；MixFile 的 `version` 恒 0 无用，HeartShard 的 version 从 1 起步用于格式演进 |
| 7 | AAD 无 | **引入 AAD = `blob_hash ‖ chunk_index ‖ total_chunks`**（仅 HeartShard 自有格式） | 防分片重排/截断/跨 blob 拼接。MixFile 裸格式作为对齐基线保留测试通道，HeartShard 生产格式带 AAD |
| 8 | 密钥单一、分片 IV 每次随机 | **每 blob 派生独立密钥** `blob_key = HKDF(MK, salt=blob_hash, info="blob-enc")`，**每 chunk 随机 12B IV**（与 MixFile 同构） | 已裁决（2026-11-01，§5 Q2）：简报原"IV = IDA_SEED+chunkIndex"在 GCM 下不同 blob 同序号 chunk 会重用 keystream；独立 blob_key 根除该风险，随机 IV 不影响 blob 级去重 |

### 2.3 兼容性立场（已裁决 2026-11-01）

HeartShard **不读取、不解析 MixFile 分享码**（`mf://`），不兼容图床存储的 MixFile 内容。
MixFile 对齐范围限定为：分片加密原语逐字节一致 + 分片模型同构。
理由：分享码绑定图床 URL 生态，与"用户自有存储"定位冲突；保留兼容会引入 MD5("123") 硬编码密码等不安全原语。

---

## 3. 测试向量

生成与验证脚本（已纳入版本控制）：`docs/fixtures/`（M0 实现时迁入 `core-rs/crates/crypto/tests/`）。

- **生成**：`docs/fixtures/VectorGen.java` — 与 `mixfile-core` 完全相同的 JCE 调用（`AES/GCM/NoPadding` + `GCMParameterSpec(96, iv)` + 无 AAD）。
- **验证 1（round-trip）**：`docs/fixtures/VectorVerify.java` — 解密还原 + base62 往返，全 PASS。
- **验证 2（独立实现交叉）**：`docs/fixtures/crosscheck.py`（Python `cryptography`/OpenSSL 后端，**与 JCE 无共享代码**）— 密文逐字节一致，全 PASS（16/16）。
- 机器可读向量：`docs/fixtures/mixfile-vectors.json`（含全部 hex 字段，M0 实现时作为 Rust 测试 fixture 原样迁入）。

固定输入（所有向量共用）：

| 名称 | 字节 | hex |
|---|---|---|
| KEY32 | 32B，`0x00..0x1f` 递增 | `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f` |
| IV_A | 12B | `a0a1a2a3a4a5a6a7a8a9aaab` |
| IV_B | 12B | `b0b1b2b3b4b5b6b7b8b9babb` |
| IV_C | 12B | `c0c1c2c3c4c5c6c7c8c9cacb` |
| PASS16 | MD5("123")，16B | `202cb962ac59075b964b07152d234b70` |

KEY32 的 base62 编码 = `03AuLtjc7TJLctqJ2Unu3mfAGcxg9lrkrCWgKbidLF`（注意首字符 `0` = 前导 `0x00` 字节，base62 往返测试用）。

### V1 — 基础向量（64B 明文，AES-256，密文全量）

| 项 | 值 |
|---|---|
| 明文（64B） | `"HeartShard M0 vector 01: MixFile AES-GCM alignment."` ASCII 循环填充 |
| 密文（88B = 12 IV + 64 ct + 12 tag） | `a0a1a2a3a4a5a6a7a8a9aaabae7d1d5f31986ade1001a79e375ab6bb13d83662b2877356bc434ffe39c21964f23702ac826510707ffd68a16e14ee9c296f680007b1680a12366a2cc050c880cf1a90863a190fd10870665f` |
| SHA-256(明文) | `sha256` hex 见 JSON `plaintext_sha256_hex`；base62 = `E6QeJU6asdY7TrVzlku1qZedQlhJLSZw256Z0GwSch1` |

### V2 — 多分片 + 索引（1.5 MiB，chunk_size = 1 MiB）

- 明文：`prng("heartshard-m0-prng-v2", 1572864)`，其中 `prng = SHA-256(seed ‖ counter32BE)` 链式展开（可复现，不占文档篇幅）。
  chunk0 明文 SHA-256 = `2708c069c04c3bd03d04efa2c970123bd3646ec55164ddd093c7447ed57525ff`，chunk1 明文 SHA-256 = `04e9c1074a48e7082ebca1864ae73c0976da1d3228548fa26549643701976736`。
- chunk0（1 MiB，IV_A）密文 sha256 = `a6fc8a206c268812a6c8df1ef051b7a53d8802992cf3e9ff68129c9a2ff7c846`（1048600 B = 12 IV + 1048576 ct + 12 tag）；
  首 32B = `a0a1a2a3a4a5a6a7a8a9aaab2fde7782ed37cce8d9b946abe02d27f507976784`，末 32B = `282f747d0941a1faf1b6082e28dc63be0f0cdd351a3c7356f80c1d8e72dc9b12`。
- chunk1（512 KiB，IV_B）密文 sha256 = `ab615166d180140a59fa5c76bef1d3971a46476aae04fdedb5ae07e11ee79669`（524312 B）；
  首 32B = `b0b1b2b3b4b5b6b7b8b9babb8974d47e67b6646efebf69ae1c044c52bdcf2358`。
- 索引 JSON：

```json
{"chunk_size":1048576,"file_size":1572864,"version":0,"file_list":["https://example.com/blob/c0#9fSRtAC4aYRRWJyeWtBFf7eGdIeWTID6zx42WklSTDh","https://example.com/blob/c1#1aeruxwZ0lZvaI5clwjpBX1QGV83pgBknD0PQt0zj0y"]}
```

- gzip（Java 输出，192B）：`1f8b08000000000000ff7dccb90e…`（完整 hex 见 JSON `index_gzip_hex`）。
- gzip → AES 加密（IV_C）后 216B 密文全量见 JSON `index_encrypted_hex`，密文 sha256 = `af82eada8b7275133143c0bb3772daa83e163850f4fdee4ed0ef3d4b2403ffcd`。

### V3 — 分享码向量（AES-128 密钥 MD5("123")，输出全量）

- 明文（MixShareInfo JSON，字段按声明序）：`{"f":"sunset-beach.jpg","s":1572864,"h":0,"u":"https://example.com/blob/index#gudSRmA4p4cEK7NDJtRlIucQUMlu3RjvqxfOQGESSS3","k":"03AuLtjc7TJLctqJ2Unu3mfAGcxg9lrkrCWgKbidLF","r":""}`
- 密钥 PASS16（16B → AES-128），IV_A。密文 86B（hex 见 JSON `ciphertext_hex`）。
- **分享码（base62，273 字符）**：

```
dNn7UA2Al0Tiim6WfJ9TfxxKDoyo4K48iW9WAPTrelnap1gdW9PaV2p3YOyXfwHTrNHhV869GUgbPUpDFvXivRbYlIq5NPyoecNyPdpxM0XepGXZSomtkqtp7LAAREVD465dhs6i0lDEKRD77BgUF567OejAwqrMOM4tWEANUavUzcH0WvJtsQr1ZcknsESO8o8Exb2yQAaAlrWlEnhRRM6lK4z127fp7OQ9vLMkDykNxrzxKffyZ2jcSEM4s9pTudWPEWJ3hBhpVDSFi
```

### V4 — 空明文边界

- 明文 0 字节，KEY32 + IV_A → 密文 24B：`a0a1a2a3a4a5a6a7a8a9aaab5c699625af4b93a0f8220a2a`（= IV ‖ 12B tag，无 ct）。

### 向量使用约定

1. Rust 加密实现对 4 组向量**逐字节复现**（`cargo test`），解密实现对全部密文还原。
2. gzip 层**不要求跨语言字节一致**（gzip 头含 OS 字节等实现差异；本例 Java 17 输出 OS=0xff，恰与 Rust `flate2` 默认一致）。跨端测试以"解压后 JSON 一致"为准，密文级向量以 Java gzip 字节为基准（Rust `flate2::write::GzEncoder` 默认参数即可复现）。
3. HeartShard 生产格式（带 AAD、派生密钥）在 M0 实现时另补独立向量，不复用本表。

---

## 4. Rust core 目录结构与依赖清单

```
core-rs/
├── Cargo.toml                  # workspace
├── crates/
│   ├── crypto/                 # AES-256-GCM 分片加解密（AesGcm<Aes256, U12>）
│   │   ├── src/lib.rs          #   encrypt_chunk / decrypt_chunk（MixFile 裸格式）+ HeartShard 带AAD格式
│   │   └── src/kdf.rs          #   Argon2id → MK → HKDF(ENC_KEY/MANIFEST_KEY/CONFIG_KEY/IDA_SEED/blob_key)
│   ├── shard/                  # chunk 切分、per-chunk/per-shard SHA-256、编解码容器
│   ├── ida/                    # Reed-Solomon(n,m) 封装 + shard 分配策略
│   ├── index/                  # 加密 SQLite（SQLCipher）模型 + Lamport 合并（M2）
│   ├── manifest/               # manifest 序列化/加密/冗余存放策略
│   └── keypath/                # 对象键生成 + sanitize_key_segment（§5.2 硬约束）
└── bindings/
    ├── wasm/                   # wasm-bindgen，供 Tauri WebView
    └── ffi/                    # ohos_napi_rs（napi-rs），供 ArkTS；全部 catch_unwind
```

### 依赖清单（crates/crypto + shard + ida 起步所需）

| crate | 版本 | 用途 | 备注 |
|---|---|---|---|
| `aes-gcm` | 0.10 | AES-256-GCM，`AesGcm<Aes256, U12>`（12B tag） | RustCrypto；no_std 兼容 WASM |
| `sha2` | 0.10 | SHA-256（chunk/blob/shard 校验） | RustCrypto |
| `hkdf` | 0.12 | MK → 子密钥派生 | RFC 5869，SHA-256 |
| `argon2` | 0.5 | 口令 → MK（m=64MiB, t=3, p=4） | RustCrypto；参数待 §5 Q4 确认 |
| `zeroize` | 1 | 会话密钥用后清零 | |
| `rand_core` / `getrandom` | 0.6 / 0.2 | IV 与密钥随机源 | WASM 下走 `getrandom` js 特性 |
| `flate2` | 1 | 索引 gzip 压缩/解压 | 默认 miniz_oxide 后端，纯 Rust，WASM 兼容 |
| `serde` + `serde_json` | 1 | manifest/索引序列化 | |
| `reed-solomon-simd` | 3.1 | RS(n,m) 纠删码 | 选型见 §4.1 |
| `thiserror` | 2 | 错误类型 | |

### 4.1 Reed-Solomon crate 选型

**选定：`reed-solomon-simd` 3.1.0（主选）；`reed-solomon-erasure` 6.0.0（备选）。**

| 维度 | reed-solomon-simd | reed-solomon-erasure |
|---|---|---|
| 算法 | Leopard-RS（GF(2^16)），O(n log n) | 经典矩阵（Backblaze JavaReedSolomon 移植），GF(2^8/2^16) |
| 性能 | 官方基准：AVX2 单核最高 ~10 GiB/s 编码，对比项中几乎全胜（解码在恢复片 ≤42 时略逊） | 慢一个数量级量级 |
| shard 数上限 | 1–32768 原 + 1–32768 恢复（最大 65535 总数） | GF(2^8) 下 ≤256 |
| SIMD | 运行时探测 Neon/SSSE3/AVX2，**纯 Rust fallback**（WASM 可用 fallback 路径） | 纯软件实现 |
| 许可 | MIT AND BSD-3-Clause | MIT |
| 维护 | 2026-10 发布 3.1.0，100% 文档化 | 2026-10 发布 6.0.0，活跃 |

选型理由：
1. **客户端性能**：相册批量导入动辄数 GB，编码耗时直接进上传路径；Leopard-RS 在 1 MB chunk、n=4 场景编码开销 <1ms 级。
2. **WASM 兼容**：带纯 Rust fallback，`wasm32-unknown-unknown` 无 SIMD 特性也能编译运行（性能退化为标量路径，仍够用）。
3. **shard 上限余量**：未来支持大文件更多分片数不受 256 限制。

**注意点（写入 manifest 的约束）：**
- Leopard-RS 的 shard 与经典 Vandermonde/Cauchy 实现（含 reed-solomon-erasure）**不互换**。manifest 必须记录 `codec = "leopard-rs" + crate 版本`，防止未来换库导致旧数据无法恢复。
- crate 自 3.0.0 起支持任意 shard 大小，但**跨版本兼容推荐 shard 大小为 64 的倍数**；HeartShard chunk 取 256 KiB–4 MiB，n 整除后 shard 天然满足（4 MiB/6 余数场景在 M0 实现中显式处理）。
- RS 不做 shard 内纠错（只纠删除）——损坏 shard 靠 manifest 中的 per-shard SHA-256 识别后剔除，这正是 §2.2 差异 #5 引入 shard 级校验的原因。

---

## 5. 未决问题清单

### 已裁决（2026-11-01）

| # | 问题 | 裁决 |
|---|---|---|
| Q2 | 分片 IV/密钥策略（简报 §6.2 与 GCM 语义冲突：全局 ENC_KEY + 确定性 IV 会导致不同 blob 同序号 chunk 的 nonce 重用） | **每 blob 独立密钥 `blob_key = HKDF(MK, salt=blob_hash, info="blob-enc")` + 每 chunk 随机 12B IV**（与 MixFile 同构） |
| Q3 | 是否兼容读取 MixFile 分享码/图床内容 | **不兼容**（§2.3） |

### 待定（不阻塞 M0 crypto 编码，实现时按建议值取）

| # | 问题 | 现状与建议 | 影响 |
|---|---|---|---|
| Q1 | 文档/社区流传"AES-128"的说法 | 源码定论：主数据 **AES-256**（32B 随机密钥）；仅**分享码层**用 MD5("123") 16B 密钥（AES-128）。无歧义，记录在案 | 无 |
| Q4 | Argon2id 参数 | MixFile 无 KDF（密钥不来自口令），无对齐对象。简报 m=64MiB, t=3, p=4 属 OWASP 建议区间，移动端 64 MiB 内存峰值需实测（低端 Android/鸿蒙可能 OOM）。按 64MiB 实现并留配置降级（32MiB） | kdf.rs |
| Q5 | 分片大小范围 | 简报 256 KiB–4 MiB vs MixFile 上限 20 MiB。维持简报值（索引容量与失败重传粒度权衡），20 MiB 不开放 | 设置 UI |
| Q6 | RS 参数默认值 n=4, m=2 | 按简报 6.3 表述"**总 shard n，恢复阈值 m**"语义：2 数据 + 2 校验，损 2 仍可恢复，存储放大 2× | manifest 格式 |

---

## 6. M0 退出标准核对

| 简报要求 | 状态 |
|---|---|
| AES 参数源码确认（256 vs 128 矛盾裁决） | ✅ §1.1，AES-256 定论，128 说法溯源到分享码层 |
| 分片大小默认/可配置/索引容量关系 | ✅ §1.2 |
| SHA256 粒度（分片级 vs blob 级、时机） | ✅ §1.3：分片级、明文、下载时 |
| 索引/manifest 结构、密钥封装方式 | ✅ §1.4：无真封装，密钥随分享码明文携带 |
| 隐写层抽象（不照搬图床 API） | ✅ §1.5 + §2.2#1：抽象为与存储无关的 `encrypt_chunk` |
| ≥3 组测试向量 | ✅ §3：4 组 MixFile + 3 组 HeartShard（Python 生成），全部逐字节锁定 |
| RS crate 选型理由 + 基准数据 | ✅ §4.1 |
| Rust 目录结构 + 依赖清单 | ✅ §4 |
| 未决问题清单 | ✅ §5（Q2/Q3 已裁决；Q1/Q4/Q5/Q6 记录在案，按建议值实现） |

### 6.1 M0 实现结果（2026-11-01）

| 退出标准 | 结果 |
|---|---|
| 测试向量逐字节匹配 | ✅ `cargo test -p crypto`：MixFile V1–V4（Java JCE 基准 + OpenSSL 交叉）+ HeartShard K1/H1/H2（argon2-cffi + OpenSSL 交叉）全过 |
| `sanitize_key_segment` 全部边界测试 | ✅ `cargo test -p keypath` 10/10 + `tools/keypath-tests` 共享向量 40/40 |
| `core-rs` crypto/shard/keypath 模块 | ✅ 四 crate：keypath / crypto / ida / shard，38 测试全绿，clippy 0 警告 |

交付物：

```
core-rs/crates/keypath   # 对象键生成 + sanitize（四端唯一实现，共享向量锁契约）
core-rs/crates/crypto    # bare 层（MixFile 逐字节）+ KDF（Argon2id→MK→HKDF）+ AAD chunk 层
core-rs/crates/ida       # Reed-Solomon n/m 分散（n=总 shard，m=恢复阈值；n==m 短路）
core-rs/crates/shard     # blob→chunk→加密→分散→重组全管线 + 存储丢失恢复测试
tools/keypath-tests      # 共享向量 JSON + Rust 校验器（其他端对照程序）
docs/fixtures/           # 两套机器可读向量 + 生成脚本（Java/Python 双来源）
```

已知限制（进入 M1 前注意）：
- RS 测试规模最大 1 MiB；M1 端到端需在本机跑 release 基准并记录 Leopard-RS 实测耗时。
- `DerivedKeys` 内部持有 MK（会话内存），`Hkdf` 的 prk 未做 Drop 零化——与会话密钥威胁模型一致，但可在 M2 加固。
- gzip 字节级跨语言一致未纳入契约（按 §3 约定以解压后内容为准）；Rust `flate2` 默认 gzip 头与 Java 17 输出（OS=0xff）实测一致。
- shard 测试里 Argon2（32MiB low profile）每次重建密钥，debug 模式约 2–3s/次——M1 引入会话级 key cache 后消除。
