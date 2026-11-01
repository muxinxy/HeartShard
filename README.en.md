# HeartShard

> **Shatter your memories into pieces — and let every cloud you trust raise them.**

[中文](README.md)

HeartShard is a **pure client-side, serverless, end-to-end encrypted** multi-cloud photo album system spanning Windows / Android / iOS / HarmonyOS. Photos and videos are split into chunks, encrypted, protected with erasure coding, and scattered across the user's own cloud storage (S3-compatible / WebDAV / OpenList / FTP). **No servers, no accounts, no centralized index** — cloud storage acts purely as a passive blob repository; all decryption, indexing, and synchronization logic runs on the client.

Built for privacy-sensitive users who want their photos on multiple drives / NAS / S3 buckets without trusting any single provider.

## Key Features

- 🔐 **End-to-end encryption**: AES-256-GCM chunk encryption with a per-blob key (HKDF-derived) and a random IV per chunk; the passphrase derives the master key via Argon2id — neither passphrase nor keys ever touch disk
- 🧩 **Sharding + erasure coding**: files split into 256 KiB–4 MiB chunks, Reed-Solomon redundancy (default n=4, m=2) — a full cloud-storage outage still allows complete recovery
- 🗂 **Directories are albums**: Windows directory import becomes an album tree inside the encrypted index; original file names and directory structure are never visible in the cloud
- 🔄 **Multi-device sync**: pure client-side Lamport-clock merging, no real-time consistency required
- ☁️ **Multi-cloud dispersion**: shards of the same blob are forced onto different Storages; S3 / WebDAV / OpenList / FTP with weights and health checks
- 📱 **One core, four platforms**: a single Rust core shared by two shells — Tauri (Windows/Android/iOS) and native ArkTS (HarmonyOS)

## Current Progress

| Milestone | Scope | Status |
|---|---|---|
| **M0** | MixFile algorithm study + core-rs (keypath / crypto / ida / shard) + locked test vectors | ✅ Done (`fe5d3e6`) |
| **M1** | Tauri Windows: upload → local-directory mock Storage → download & restore → SHA-256 verify | ✅ Done (E2E green); the §8.5 five-phase directory-import wizard is later UI work |
| **M2** | S3 adapter (SigV4) + SQLCipher index + two-device Lamport sync | ✅ Done (timelines converge in E2E; conflict rulings unit-tested) |
| **M3** | **Android** Tauri shell (aarch64 APK) + WebDAV adapter + background transfer | ✅ Shell/WebDAV/foreground service (notification keeps process alive during imports) done; long-run on-device soak pending a real device |
| **M4** | HarmonyOS ArkTS shell + Rust FFI + OpenList adapter | OpenList adapter ✅ (mock covers all endpoints; live-instance acceptance pending); HarmonyOS shell ⚠️ requires DevEco/OHOS SDK (unavailable here), see [dev guide](docs/development.en.md) |
| **M5** | iOS + FTP/FTPS + repair/rebalance + threat-model document | Repair ✅ E2E, rebalance ✅ tested, FTP/FTPS ✅ adapter (live acceptance pending), [threat model](docs/threat-model.md) ✅; iOS ❌ requires macOS |
| **Artifacts** | Windows portable exe + installers; Android APK (aarch64) | ✅ `tauri build` / `tauri android build --apk` |

> Platform priority (adjusted 2026-11-01): **Windows → Android first**; iOS and HarmonyOS follow.
> Honest gaps against the brief: directory-import wizard & history UI (§8.5), task center /
> virtual-scrolled timeline / map views (§10), and §12.2 scenarios such as "5000+ files",
> "offline resume", "HarmonyOS 100MB WebDAV chunked upload".

## Getting Started (Development)

Requirements: [mise](https://mise.jdx.dev) for toolchains; pwsh recommended as the terminal.

```powershell
git clone <repo-url> heartshard; cd heartshard
mise install          # rust stable + node 24 + pnpm
cargo test --manifest-path core-rs/Cargo.toml --workspace   # 38 tests should pass
cargo run --manifest-path tools/keypath-tests/Cargo.toml    # shared keypath vectors (40 cases)
```

See the [Development Guide](docs/development.en.md) for environment setup, architecture conventions, the test-vector provenance chain, and commit discipline.

## Repository Layout

```
heartshard/
├── core-rs/                    # Rust core shared by all four platforms (built as WASM / cdylib)
│   └── crates/
│       ├── keypath/            # object-key generation + filename validation (single source of truth)
│       ├── crypto/             # AES-256-GCM chunk encryption + Argon2id/HKDF key derivation
│       ├── ida/                # Reed-Solomon information dispersal
│       └── shard/              # blob → chunk → encrypt → disperse → reassemble pipeline
├── apps/                       # platform shells (from M1)
├── tools/keypath-tests/        # cross-platform keypath consistency tests (shared vectors + checker)
├── docs/                       # research & development documents
│   ├── mixfile-study.md        # MixFile algorithm study (parameters, vectors, design rulings)
│   ├── fixtures/               # machine-readable test vectors + generators (Java/Python sources)
│   └── development.md          # development guide (Chinese; English: development.en.md)
└── .research/                  # upstream reference clones (gitignored)
```

## Security Model (Summary)

- A cloud provider sees only: encrypted shards, unreadable hash-based object keys, encrypted manifests and index
- The passphrase never touches disk; the master key lives in session memory only; sensitive storage credentials are encrypted with derived keys at rest
- Log redaction: no shard contents, no full URL queries, no keys
- Integrity chain: per-shard SHA-256 → RS reassembly → GCM tag/AAD (binding blob identity & chunk position) → per-chunk and blob-level SHA-256

The full threat model ships with M5; algorithm alignment and design rulings are documented in [docs/mixfile-study.md](docs/mixfile-study.md).

## License

TBD.
