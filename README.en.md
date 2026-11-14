<div align="center">

<img src="apps/tauri/src-tauri/icons/icon.png" width="128" alt="HeartShard icon"/>

# HeartShard

**Shatter your memories into pieces — and let every cloud you trust raise them.**

[简体中文](README.md) | [English](README.en.md)

</div>

HeartShard is an end-to-end encrypted family photo album for Windows, Android
and the Web. Use it **pure client-side** (no server, no accounts — talking
straight to your own cloud storage) or connect it to a **self-hosted server**
for multi-user, web access and share links. Photos/videos are sharded,
encrypted and scattered across your own S3-compatible / WebDAV / OpenList /
FTPS storage — the cloud and the server learn nothing.

## Contents

- [Features](#features)
- [Two ways to use](#two-ways-to-use)
- [Docker deployment](#docker-deployment)
- [Quick start (dev)](#quick-start-dev)
- [Repository layout](#repository-layout)
- [License](#license)

## Features

- **End-to-end encryption**: per-chunk AES-256-GCM, Leopard-RS erasure coding
  (default n=4/m=2), SHA-256 everywhere; the password derives the master key
  via Argon2id and never leaves the device
- **Album-first UI**: timeline, grid, album tree, favorites, restorable trash,
  map, day/year-month navigation — crypto/sharding details tucked away
- **Privacy sharing**: album or multi-photo links with **EXIF/GPS metadata
  stripped by default**, optional expiry & access code; WebCrypto decryption,
  the server only ever sees ciphertext
- **Optional self-hosted server**: argon2id auth, album-level permissions
  (hidden/view/upload/manage with inheritance), TOTP 2FA, sync sources
- **Multi-device sync**: Lamport clocks + LWW merge + delta; syncer CLI for
  local dirs and cloud sources (append/mirror strategies)
- **Motion photos**: Android Motion Photo (trailing MP4) detection,
  extraction, playback and two-file export; Apple Live Photo pairing
- **Multi-cloud**: shards of one blob land on different storages; S3
  (hand-written SigV4), WebDAV, OpenList (token), FTPS

## Two ways to use

### 1. Pure client-side (serverless)

Add your cloud storage in the desktop app; the data dir lives on your
machine — no server, no accounts. Share the `mk.wrap` key file across
devices for sync.

### 2. Self-hosted server

- Multi-user + album ACL + TOTP 2FA + share links
- Switch the desktop lock screen to "Connect to server"; open the server URL
  in a mobile browser to install the PWA
- See [server.toml.example](server/server.toml.example) and the
  [development guide](docs/development.md) (Chinese) for details

## Docker deployment

The server ships a multi-arch image (amd64 / arm64 / armv7):

```bash
docker pull ghcr.io/muxinxy/heartshard-server:v0.1.0

docker run -d -p 8420:8420 \
  -v heartshard-lib:/data/library \
  -v heartshard-meta:/data/meta \
  -e HEARTSHARD_LIB_PW=your-library-passphrase \
  ghcr.io/muxinxy/heartshard-server:v0.1.0
```

Or use [docker-compose.yml](docker-compose.yml): set `HEARTSHARD_LIB_PW` in
`.env`, then `docker compose up -d` and open `http://nas:8420`.

## Quick start (dev)

```powershell
git clone https://github.com/muxinxy/HeartShard.git heartshard; cd heartshard
mise install          # rust stable + node 24 + pnpm
cargo test --manifest-path core-rs/Cargo.toml --workspace
```

Build artifacts:

```powershell
cd apps\tauri; pnpm tauri build                 # Windows portable exe
.\scripts\build-android-apk.ps1                 # Android APK (arm64)
cd server; cargo build --release                # server binary
```

See the [development guide](docs/development.md) (Chinese) and the
[changelog](CHANGELOG_EN.md) for more.

## Repository layout

```
heartshard/
├── core-rs/crates/             # Rust core: keypath / crypto / ida / shard / index
│   └── heart-core/             # platform-agnostic library: HeartCore + cloud adapters
├── apps/tauri/                 # Windows + Android client (Tauri 2 + React)
├── server/                     # self-hosted server (axum: users/ACL/sync/share/PWA)
├── tools/syncer/               # album sync CLI (append/mirror strategies)
├── scripts/                    # build scripts (Android APK pipeline)
├── docs/                       # dev guide / threat model / algorithm research
└── .github/workflows/          # multi-platform, multi-arch CI
```

## License

[MIT](LICENSE)
