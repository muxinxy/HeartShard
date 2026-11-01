# HeartShard Development Guide

[中文](development.md)

> Engineering guide for contributors to this repository. For product positioning and the security model see the [README](../README.en.md); for algorithm research see [mixfile-study.md](mixfile-study.md).

## 1. Environment Setup

### 1.1 Toolchains (managed by mise)

Toolchain versions are pinned via [mise](https://mise.jdx.dev) (see the root `mise.toml`): rust stable (≥1.85, edition 2024), node 24, pnpm.

```powershell
# One-time setup (pwsh)
winget install jdx.mise          # if mise is not installed
mise trust                       # trust the project mise.toml
mise install                     # install rust/node/pnpm
rustup target add wasm32-unknown-unknown   # for WASM bindings (from M1)
```

Verify:

```powershell
mise exec -- rustc --version   # ≥ 1.98
cargo test --manifest-path core-rs/Cargo.toml --workspace   # 38 tests green
```

### 1.2 Terminal note (important)

- **Contributor terminal**: this project standardizes on **pwsh**. Commands in docs and issues are pwsh syntax (`$env:VAR="x"`, not `VAR=x cmd`).
- **CI / some automation tools**: on Windows may fall back to cmd.exe (with BusyBox utilities on PATH). When writing automation scripts, do not rely on `;` as a command separator (unsupported in cmd) or `for` loops; use `&&` chains or script files instead.

### 1.3 Rust targets

| target | Purpose | Needed from |
|---|---|---|
| `x86_64-pc-windows-msvc` | host dev/testing (default) | M0 |
| `wasm32-unknown-unknown` | Tauri WebView WASM bindings | M1 |
| `aarch64-linux-android` etc. | Android (NDK) | M3 |
| `aarch64-apple-ios` | iOS | M5 |
| `aarch64-unknown-linux-ohos` | HarmonyOS cdylib | M4 (rust ≥1.78) |

## 2. Repository Layout & Module Boundaries

```
core-rs/                     # Rust core workspace
├── crates/keypath/          # object-key generation + sanitize_key_segment — single source of truth; shells must not reimplement
├── crates/crypto/           # bare layer (MixFile-aligned) / KDF / AAD chunk layer
├── crates/ida/              # Reed-Solomon dispersal (Leopard-RS wrapper)
├── crates/shard/            # blob→chunk→encrypt→disperse→reassemble pipeline
└── bindings/                # wasm/ (Tauri) and ffi/ (HarmonyOS napi) — added in M1/M4
apps/tauri/                  # Windows + Android + iOS UI shell (from M1)
apps/harmony/                # HarmonyOS ArkTS shell (from M4)
tools/keypath-tests/         # cross-platform keypath contract: vectors.json + Rust checker
docs/fixtures/               # machine-readable test vectors + generators (Java/Python sources)
.research/                   # upstream reference clones (gitignored, never committed)
```

Hard boundaries (non-negotiable):

1. `sanitize_key_segment` and object-key generators live **only** in `core-rs/crates/keypath`; every new platform must prove consistency against the shared vectors (`tools/keypath-tests/vectors.json`).
2. The cloud only ever sees hash-based object keys — timestamps, sequential IDs, and original file names are forbidden in any object key. User-controlled strings such as album names live exclusively in the encrypted index.
3. Logs never contain shard contents, full URL queries, keys, or passphrases.

### 2.1 M1 integration (Windows)

How `apps/tauri` (Tauri 2 + React + TS + Vite) wires into the core:

- **src-tauri depends on the four core-rs crates directly via path deps** (`crypto/shard/keypath/ida`); Tauri commands do all encryption/sharding/persistence on the Rust side — the "no WASM layer" path allowed by the brief (§4.1). The WASM binding (`core-rs/bindings/wasm`) is deferred until in-page computation is needed; the "one core" principle is unaffected.
- `src-tauri/src/library.rs` is the **pure business layer** (no Tauri types; directly testable): session keys (`unlock`/`lock`, MK in session memory only), local-directory mock Storage, encrypted manifests, and the M1 encrypted-JSON index.
- `src-tauri/src/commands.rs` is the thin Tauri command layer (`Mutex<HeartCore>` session state).
- M1 known simplifications (absorbed in M2+): whole-file in-memory reads (streaming comes with the import wizard); single manifest copy (≥2 redundant copies once S3 lands in M2); local index is a MANIFEST_KEY-encrypted JSON (replaced by the SQLCipher index.db in M2); MIME inferred from extension (EXIF in M1.5).

## 3. Building & Testing

```powershell
cargo test --manifest-path core-rs/Cargo.toml --workspace     # all unit/vector tests
cargo run --manifest-path tools/keypath-tests/Cargo.toml      # shared keypath vectors (40 cases)
cargo clippy --manifest-path core-rs/Cargo.toml --workspace --all-targets   # 0-warning baseline
```

### 3.1 Test-vector provenance chain

Vectors are not invented — they are cross-validated by two independent implementations, then locked by Rust:

```
Java VectorGen.java (same JCE calls as mixfile-core)
        │  generates mixfile-vectors.json (V1–V4)
        ▼
Python crosscheck.py (OpenSSL backend, byte-for-byte comparison) — all PASS
        │
        ▼
Rust core-rs/crates/crypto/tests/vectors.rs (byte-for-byte assertions)

Python gen-heartshard-vectors.py (cryptography + argon2-cffi)
        │  generates heartshard-vectors.json (K1/H1/H2: Argon2id, HKDF, AAD ciphertext)
        ▼
Rust asserts the same
```

Run the vector tests before touching the crypto layer; the vector files are contracts — changes require updating all three generator/consumer sides with justification.

## 4. Core Design Decisions (Quick Reference)

Details and source evidence in [mixfile-study.md](mixfile-study.md); conclusions only here:

| Decision | Conclusion |
|---|---|
| Chunk encryption | AES-256-GCM, 12B random IV, 96-bit tag, wire format `iv‖ct‖tag` |
| Key chain | passphrase → Argon2id(m=64MiB, t=3, p=4; low-end 32MiB) → MK → HKDF: `manifest_key` / `config_key` / `blob_key` |
| blob_key (ruling Q2) | `HKDF(MK, salt=master_salt, info="heartshard/blob/v1"‖blob_hash)`; random IV per chunk eliminates GCM nonce reuse |
| AAD | `blob_hash(32B) ‖ chunk_index(u32 BE) ‖ total_chunks(u32 BE)` |
| MixFile compatibility (ruling Q3) | None: no `mf://` share-code parsing, no image-host content |
| Erasure coding (ruling Q6) | n = total shards, m = recovery threshold (default 4/2, 2× storage amplification); Leopard-RS; codec id recorded in manifest |
| Object keys | `blobs/{h[0:2]}/{h[2:4]}/{hash}/{chunk:05}_{shard:02}.shard`; manifests likewise; 65,536 two-level hex buckets |
| Chunk size | 256 KiB–4 MiB (default 1 MiB); three-layer override global < storage < upload session; immutable once in manifest |
| Objects per directory | limit 5,000 (2,000 recommended for FTP); warn only, never auto-migrate |
| Album tree | lives only in the encrypted index; directory import truncates at depth 8; `import_source` is local_only |

## 5. Milestones (platform priority adjusted 2026-11-01)

**Windows and Android come first**: Android is split out of the original combined "Android/iOS" milestone and moved up; iOS moves to M5.

| Milestone | Deliverables | Exit criteria |
|---|---|---|
| **M1** | Tauri Windows: upload → local-directory mock Storage → download & restore → SHA-256 verify | runnable end-to-end, verification passes |
| **M2** | S3 adapter + two-device index-sync simulation | identical timelines on both devices; correct Lamport conflict resolution |
| **M3** | **Android** Tauri shell + background transfer (Foreground Service) + WebDAV adapter | uploads survive backgrounding; WebDAV large-file chunked resume |
| **M4** | HarmonyOS ArkTS shell + `ohos_napi_rs` + OpenList adapter | HarmonyOS upload/download/browse; no panic crosses FFI |
| **M5** | iOS shell + FTP/FTPS + repair/rebalance + threat-model document | feature parity across platforms; repair E2E passes |

Each milestone ends with: runnable commands, known platform limitations, next-milestone prerequisites, screenshots or curl samples.

## 6. Commit Discipline

1. **Verify external APIs with minimal scripts before writing adapters** — run real services (curl/PowerShell) to confirm fields and behavior; never hard-code from memory.
2. Commit per milestone; every commit stays runnable (tests green).
3. Resolve parameter ambiguity from source/measurement, record evidence in `docs/`, never guess.
4. Before committing: `cargo test --workspace` + frontend `pnpm test` / `pnpm typecheck` (from M1).
5. HarmonyOS FFI boundary: every Rust function exposed to ArkTS must wrap `catch_unwind` and return error codes — panics must never cross.

## 7. Known Limitations & Deferred Work

- RS benchmarks are crate-official; M1 must measure locally in release mode (1 MiB chunk, n=4).
- `DerivedKeys` holds the MK (session memory) and the `Hkdf` prk is not zeroized on drop → harden in M2.
- shard tests rebuild Argon2 keys each time (~2-3s in debug) → introduce a session key cache in M1.
- Byte-identical gzip across languages is not part of the contract (compare decompressed content); flate2 and Java 17 headers match in practice (OS=0xff).
