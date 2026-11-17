# Changelog

All notable changes to HeartShard are documented here.
中文版见 [CHANGELOG.md](CHANGELOG.md).

## [0.2.1] - 2026-11-17

Mobile and experience fixes.

### Fixed

- The topbar avatar now updates immediately after changing the avatar (CSS
  url() quoting bug made the background assignment a no-op)
- Mobile topbar: search wraps to its own row; avatar / upload / refresh no
  longer pushed off screen; the upload button no longer stacks vertically
- Shares / trash tables scroll horizontally on narrow screens; the share
  content dialog drops the "metadata stripped" column and keeps date/preview
  on one line
- Lightbox top bar buttons scroll horizontally and are all reachable on
  mobile
- Desktop video thumbnails: the grid and trash render a real first-frame
  thumbnail via a `<video>` element (no more placeholder icons)
- The web client shows first-frame video thumbnails as well

## [0.2.0] - 2026-11-17

Full desktop client redesign: information architecture, visuals and features
now match the web client; server-side management APIs and hardening; official
multi-arch Docker images and map-based place exploration.

### Added

**Desktop client (redesigned to match the web client)**
- Sidebar information architecture: timeline / explore / albums / shares /
  favorites / trash + admin (users / groups / sources / storage / status),
  with hash routing for deep links
- Explore: category entries + GPS place clustering; clicking a place opens a
  map with stacked thumbnails and drill-down
- Albums: cover wall + sorting + right-click rename / set cover (pick a photo
  instead of pasting a hash) + ACL editor
- Shares and admin area (server mode); related entries hidden automatically
  in local mode
- Topbar search, upload/import button, avatar menu (avatar / password / 2FA /
  theme / lock)
- Drawer sidebar: collapses on narrow screens with a hamburger button
  (mobile / narrow window support)
- A per-page refresh button; forms are stacked with a max width
- Login slider captcha (rc-slider-captcha) + TOTP 2FA
- All emoji replaced with the minimal line SVG icons shared with the web

**Server**
- Login rate limiting: 5 consecutive failures per IP locks it for 10 minutes
- 2FA is now "enabled after confirmation": setup no longer persists the
  secret; added `POST /api/me/totp/enable` with server-side verification;
  QR code in the bind dialog; admins can reset any member's 2FA
  (`POST /api/admin/users/{uid}/reset-totp`)
- The user list no longer leaks password hashes / TOTP secrets; only status
  flags are exposed
- Album management API: rename / set cover (manage permission)
- System settings API: `GET/POST /api/admin/settings` (chunk size / n / m /
  download concurrency)
- Storage probe: `POST /api/admin/storage/probe` to test connectivity before
  adding
- CORS enabled for cross-origin clients (desktop webview)
- Unfiled photos are visible to logged-in users (fixes thumbnail/raw 403)

**Deployment**
- Multi-arch Docker image: `ghcr.io/muxinxy/heartshard-server`
  (amd64 / arm64 / armv7; tag builds publish both the version and latest)

### Fixed

- Desktop "connect to server" routed to local unlock (dynamic Proxy dispatch)
- Dual-mode capability flags are now read live (share/2FA entries no longer
  leak into local mode)
- Duplicate uploads count as "skipped" instead of failing
- Timeline not refreshing after upload/import (GET cache busting)
- Album covers fall back to the first photo when the cover photo is deleted
- Unfiled photos thumbnail/raw 404 (visibility and video placeholders)
- Lightbox toolbar no longer covers video controls
- Web slider position math and styling; desktop verification now checks the
  slider travel

### Changed
- Forms are stacked with a max width; button radius unified at 10px
- System status is now a chart dashboard (bar + media composition donut)

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

[0.2.1]: https://github.com/muxinxy/HeartShard/releases/tag/v0.2.1
[0.2.0]: https://github.com/muxinxy/HeartShard/releases/tag/v0.2.0
[0.1.0]: https://github.com/muxinxy/HeartShard/releases/tag/v0.1.0
