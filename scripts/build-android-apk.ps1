# HeartShard Android APK 完整构建（Windows 宿主）。
# 背景：tauri CLI 的 android build 在 symlink 一步必然失败（Windows 无符号链接
# 特权），而其内部顺序是 cargo → symlink → 资产同步 → gradle，导致**资产从未
# 入包**（APK 白屏）。本脚本把整条链固化：cargo 交叉编译 + 手动拷贝 .so 与
# 前端资产 + gradle 打包。
#
# 用法（pwsh，工具链走 mise）：
#   .\scripts\build-android-apk.ps1            # 完整构建（cargo + gradle）
#   .\scripts\build-android-apk.ps1 -SkipRust  # 仅重打包（前端改动后）
# 产物：dist\HeartShard-0.1.0-arm64.apk
param(
    [switch]$SkipRust
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tauriDir = Join-Path $repoRoot "apps\tauri"
$genDir = Join-Path $tauriDir "src-tauri\gen\android"
$assetsDir = Join-Path $genDir "app\src\main\assets"
$jniLibs = Join-Path $genDir "app\src\main\jniLibs\arm64-v8a"
$soPath = Join-Path $repoRoot "target\aarch64-linux-android\release\libtauri_lib.so"
$apkOut = Join-Path $genDir "app\build\outputs\apk\arm64\release\app-arm64-release.apk"

if (-not $SkipRust) {
    # openssl 交叉编译环境：MSYS2 perl（unix 路径语义）+ NDK + GNU make
    $env:OPENSSL_SRC_PERL = "D:\Scoop\apps\msys2\current\usr\bin\perl.exe"
    $env:ANDROID_NDK_ROOT = "D:\Dev\mise\installs\android-sdk\23.0\ndk\28.2.13676358"
    $env:PATH = "D:\Scoop\apps\msys2\current\usr\bin;" + $env:PATH
    Push-Location $tauriDir
    try {
        # symlink 步必然失败（Windows 特权），cargo 产物在失败前已就绪 → 忽略退出码
        pnpm tauri android build --target aarch64 2>&1 | Out-Null
    } finally {
        Pop-Location
    }
    if (-not (Test-Path $soPath)) { throw "libtauri_lib.so 未生成，cargo 交叉编译失败" }
    New-Item -ItemType Directory -Force -Path $jniLibs | Out-Null
    Copy-Item $soPath (Join-Path $jniLibs "libtauri_lib.so") -Force
}

# 前端资产同步（tauri CLI 因 symlink 失败从未做过的一步）
Push-Location $tauriDir
try {
    pnpm build 2>&1 | Out-Null
} finally {
    Pop-Location
}
New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null
Copy-Item (Join-Path $tauriDir "dist\*") $assetsDir -Recurse -Force

# gradle 打包
$env:JAVA_HOME = "D:\Dev\mise\installs\java\17.0.2"
$env:HEARTSHARD_SKIP_RUST_TASK = "1"
$env:TAURI_ANDROID_KEYSTORE_PATH = Join-Path $tauriDir "src-tauri\gen\heartshard-release.keystore"
$env:TAURI_ANDROID_KEYSTORE_PASSWORD = "heartshard-2026"
$env:TAURI_ANDROID_KEY_ALIAS = "heartshard"
$env:TAURI_ANDROID_KEY_PASSWORD = "heartshard-2026"
Push-Location $genDir
try {
    & .\gradlew.bat assembleArm64Release --no-daemon
} finally {
    Pop-Location
}
if ($LASTEXITCODE -ne 0) { throw "gradle 构建失败" }

# 验证资产确实入包（防止空壳 APK 回归）
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($apkOut)
try {
    $hasIndex = $zip.Entries | Where-Object { $_.FullName -eq "assets/index.html" }
} finally {
    $zip.Dispose()
}
if (-not $hasIndex) { throw "APK 内没有前端资产（assets/index.html 缺失）——空壳包！" }

$dist = Join-Path $repoRoot "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item $apkOut (Join-Path $dist "HeartShard-0.1.0-arm64.apk") -Force
$hash = (Get-FileHash (Join-Path $dist "HeartShard-0.1.0-arm64.apk") -Algorithm SHA256).Hash.Substring(0, 12)
Write-Output "OK dist\HeartShard-0.1.0-arm64.apk sha256:$hash"
