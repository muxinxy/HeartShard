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
$content = $content.Replace($needle, $replacement)

# 补丁 2：CC/AR/RANLIB 路径统一正斜杠。Windows 宿主上 msys make 经 sh 执行
# Makefile 配方，反斜杠会被当转义符吞掉（D:\Dev\... → D:Devmise...）；
# 正斜杠在 cmd 与 sh 下均可执行。
$ccNeedle = @'
            let mut cc_env = compiler.cc_env();
            if cc_env.is_empty() {
                cc_env = compiler.path().to_path_buf().into_os_string();
            }
            configure.env("CC", cc_env);
'@
$ccReplacement = @'
            let mut cc_env = compiler.cc_env();
            if cc_env.is_empty() {
                cc_env = compiler.path().to_path_buf().into_os_string();
            }
            // HeartShard 补丁：Windows 宿主上 msys make 经 sh 执行 Makefile 配方，
            // 会把反斜杠路径当转义符吞掉；统一转成正斜杠（cmd 与 sh 均可执行）。
            let cc_str = cc_env.to_string_lossy().replace('\\', "/");
            configure.env("CC", cc_str);
'@
if (-not $content.Contains($ccNeedle)) {
    throw "patch anchor not found (CC) in lib.rs - openssl-src version changed; review manually"
}
$content = $content.Replace($ccNeedle, $ccReplacement)

$arNeedle = @'
            let ar = cc.get_archiver();
            configure.env("AR", ar.get_program());
'@
$arReplacement = @'
            let ar = cc.get_archiver();
            let ar_str = ar.get_program().to_string_lossy().replace('\\', "/");
            configure.env("AR", ar_str);
'@
if (-not $content.Contains($arNeedle)) {
    throw "patch anchor not found (AR) in lib.rs - openssl-src version changed; review manually"
}
$content = $content.Replace($arNeedle, $arReplacement)

$ranlibNeedle = @'
            configure.env("RANLIB", args.join(OsStr::new(" ")));
'@
$ranlibReplacement = @'
            let ranlib_str = args
                .join(OsStr::new(" "))
                .to_string_lossy()
                .replace('\\', "/");
            configure.env("RANLIB", ranlib_str);
'@
if (-not $content.Contains($ranlibNeedle)) {
    throw "patch anchor not found (RANLIB) in lib.rs - openssl-src version changed; review manually"
}
$content = $content.Replace($ranlibNeedle, $ranlibReplacement)

Set-Content -Path $lib -Value $content -NoNewline -Encoding utf8
Write-Output "patched: $lib"
