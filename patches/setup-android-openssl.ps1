# HeartShard: 从本地 cargo registry 复制 openssl-src 并打 android no-asm 补丁。
# 背景：openssl 交叉编译到 android 时，perl 生成 ARM 汇编在 msys 环境下不稳定
# （SIGPIPE/路径转换），no-asm 走纯 C 路径功能等价。详见 docs/development.md §8.2。
# 用法（pwsh）：.\patches\setup-android-openssl.ps1
# 已由 .cargo/config.toml + apps/tauri/src-tauri/Cargo.toml 的 [patch.crates-io] 引用。

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$dst = Join-Path $repoRoot "patches\openssl-src"

if (Test-Path (Join-Path $dst "src\lib.rs")) {
    Write-Output "openssl-src patch already in place: $dst"
    exit 0
}

$registry = Get-ChildItem "$env:USERPROFILE\.cargo\registry\src" -Directory | ForEach-Object {
    Get-ChildItem $_.FullName -Directory -Filter "openssl-src-300.*"
} | Sort-Object Name -Descending | Select-Object -First 1

if (-not $registry) {
    throw "openssl-src not found in cargo registry; run 'cargo fetch' once first"
}
Write-Output "copying $($registry.FullName) -> $dst"
Copy-Item $registry.FullName $dst -Recurse -Force
Remove-Item (Join-Path $dst ".cargo-ok"), (Join-Path $dst ".cargo_vcs_info.json") -Force -ErrorAction SilentlyContinue

# 打补丁：android 分支追加 no-asm
$lib = Join-Path $dst "src\lib.rs"
$content = Get-Content $lib -Raw
$needle = @'
        if target.contains("android") {
            configure.arg("no-stdio");
        }
'@
$replacement = @'
        if target.contains("android") {
            configure.arg("no-stdio");
            // HeartShard 补丁：android 交叉编译在 Windows 宿主上依赖 perl/make 生成
            // ARM 汇编（.S），在 msys 环境下 SIGPIPE/路径转换极不稳定；no-asm 让
            // OpenSSL 走纯 C 路径，功能等价（性能略降，可接受）。
            configure.arg("no-asm");
        }
'@
if (-not $content.Contains($needle)) {
    throw "patch anchor not found in lib.rs - openssl-src version changed; review manually"
}
Set-Content -Path $lib -Value $content.Replace($needle, $replacement) -NoNewline -Encoding utf8
Write-Output "patched: $lib"
