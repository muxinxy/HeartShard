# Changelog

All notable changes to HeartShard are documented here.
中文版见 [CHANGELOG.md](CHANGELOG.md)。

## [0.1.0] - 2026-11-13

First public release. A client-side, end-to-end encrypted multi-cloud photo
album: photos/videos are sharded, encrypted with AES-256-GCM, redundantly
erasure-coded (Reed-Solomon), and scattered across the user's own
S3 / WebDAV / OpenList / FTPS storage — zero-knowledge for both the optional
server and the cloud storage.

### Added

**Security**
- Slider captcha on web login (drag-to-unlock, blocks bot submissions)
- TOTP 2FA (RFC 6238, covered by official test vectors): bind/disable via the
  avatar menu on the web client; login dynamically prompts for the code

**Core**
- Encrypted sharding pipeline: per-chunk AES-256-GCM (96-bit tag),
  Leopard-RS erasure coding (n=4/m=2), SHA-256 verified throughout
- Master key wrapping: random MK wrapped by a password-derived KEK
  (password change without re-uploading shards)
- Multi-device incremental sync: Lamport clocks + LWW merge + delta publish
- Cloud storage adapters: S3 (hand-written SigV4), WebDAV, OpenList
  (token or credentials), FTPS

**Server**
- Self-hosted server: multi-user auth (argon2id), album-level ACL
  (none/view/upload/manage with inheritance, override, user-over-group),
  resident sync engine, data API
- Metadata storage: SQLite (default) or MySQL, with automatic migration
  from legacy JSON config
- Web admin: users, groups, sync sources and cloud storage all configurable
  in the browser; storage connectivity probe
- Sharing: album or multi-select photo links with **EXIF/GPS metadata
  stripped by default**, optional expiry and access code; decrypted via
  WebCrypto in the browser — the server never sees plaintext
- PWA web client: timeline, explore (GPS place clustering + type facets),
  albums, favorites, trash, multi-select batch actions, context menus,
  lightbox (zoom, rotate, flip, video playback)
- Motion photos: Android Motion Photo (JPEG trailing MP4) detection and
  video extraction; Apple Live Photo filename pairing; display, playback
  and two-file export on desktop and web
- Deployment: Dockerfile + docker-compose + multi-arch Docker image
  (amd64 / arm64 / armv7); GitHub Actions multi-platform, multi-architecture
  builds (linux x86_64/aarch64/armv7/i686, windows x86_64, android
  arm64-v8a/armeabi-v7a/x86_64)

**Desktop & Android**
- Windows portable single-file exe (data dir next to the exe) + Android APK
- Dual mode: local library (serverless, direct cloud access) or connect to
  a self-hosted server
- Timeline (day grouping + year/month rail), grid, album wall, favorites,
  restorable trash, EXIF parsing, map view, encrypted in-place video
  streaming (Range requests)
- End-to-end debug logging ([http]/[auth]/[upload]/[meta], etc.)

### Fixed
- Mode switch used a frozen provider — connecting to the server still routed
  to local unlock; now dispatched dynamically so switching takes effect
  immediately
- `album_id` after the file field was dropped during multipart parsing,
  sending every upload to "unfiled"; all fields are now read before binding
- Photo/album context menus were dead on Windows (missing CSS + stringified
  closures)
- Stale modals and selection bars lingered after sharing; route now clears
  them
- Repaired blobs still showed "degraded": manifest.storage_id was not
  rewritten when repair relocated shards

### Changed
- All user-facing messages localized to Chinese
- UI follows "album-first, tech-hidden": encryption/sharding details tucked
  away, interactions aligned with mainstream photo apps

### Known Limitations
- HEIC still images are not decoded yet (Live Photo key images are stored
  as-is); no face/scene AI classification
- Video shares keep metadata; `/raw` video loads fully into memory
- iOS requires macOS/Xcode; HarmonyOS shell requires DevEco (see
  docs/development.md, Chinese)

[0.1.0]: https://github.com/muxinxy/HeartShard/releases/tag/v0.1.0
