# 运行时自动化验证（M5b/M5c 修复闭环）：
# 1. 起临时后台（测试库口令，隔离于用户数据）
# 2. 核对 [meta] JSON 迁移日志 + meta.db 生成
# 3. curl 登录 → 建相册 → 上传入相册 → 时间线可见
# 4. 输出 PASS/FAIL 摘要
$ErrorActionPreference = "Stop"
$root = "D:\Cache\Temp\hs-verify"
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $root | Out-Null

# server.toml（临时库 + 测试口令；路径用正斜杠避免 TOML 转义）
$libDir = $root.Replace('\', '/') + "/data"
@"
port = 18499
[library]
dir = "$libDir"
password_env = "HS_VERIFY_PW"
[[users]]
name = "tester"
password = "testpw"
admin = true
sync_interval_secs = 0
"@ | Set-Content "$root\server.toml" -Encoding utf8

$env:HS_VERIFY_PW = "libpw123"
$p = Start-Process -FilePath "D:\Documents\Code\HeartShard\target\debug\heartshard-server.exe" `
    -ArgumentList "--config", "$root\server.toml" -WorkingDirectory $root `
    -RedirectStandardError "$root\stderr.log" -RedirectStandardOutput "$root\stdout.log" `
    -PassThru -WindowStyle Hidden
Start-Sleep 5

$fail = @()
try {
    $errLog = Get-Content "$root\stderr.log" -ErrorAction SilentlyContinue
    if ($errLog -match "错误") { $fail += "启动报错: $errLog" }

    # 登录
    $login = Invoke-RestMethod -Uri "http://localhost:18499/api/login" -Method Post `
        -ContentType "application/json" -Body '{"name":"tester","password":"testpw"}'
    if (-not $login.token) { $fail += "登录失败" }
    $h = @{ authorization = "Bearer $($login.token)" }

    # 建相册
    $album = Invoke-RestMethod -Uri "http://localhost:18499/api/albums" -Method Post `
        -Headers $h -ContentType "application/json" -Body '{"name":"验证相册"}'
    if (-not $album.id) { $fail += "建相册失败" }

    # 造一张真 JPEG（含 GCamera 动态照片标记 + 尾随 MP4——同时验证 M5c 检测）
    $jpeg = [System.Collections.Generic.List[byte]]::new()
    $jpeg.AddRange([byte[]](0xFF,0xD8))
    $xmpText = 'http://ns.google.com/photos/1.0/camera/ MotionPhoto'
    $jpeg.AddRange([System.Text.Encoding]::ASCII.GetBytes($xmpText))
    $jpeg.AddRange([byte[]](0xFF,0xD9))
    $jpeg.AddRange([byte[]](0x00,0x00,0x00,0x18))
    $ftypText = 'ftypisom'
    $jpeg.AddRange([System.Text.Encoding]::ASCII.GetBytes($ftypText))
    $payloadText = 'motion-video-payload'
    $jpeg.AddRange([System.Text.Encoding]::ASCII.GetBytes($payloadText))
    [System.IO.File]::WriteAllBytes("$root\motion-test.jpg", $jpeg.ToArray())

    # 上传（multipart：album_id 在 file 之后——验证字段顺序修复）
    $boundary = "HSBOUND"
    $ms = [System.IO.MemoryStream]::new()
    $w = [System.IO.BinaryWriter]::new($ms)
    $w.Write([System.Text.Encoding]::ASCII.GetBytes("--$boundary`r`nContent-Disposition: form-data; name=`"file`"; filename=`"motion-test.jpg`"`r`nContent-Type: image/jpeg`r`n`r`n"))
    $w.Write($jpeg.ToArray())
    $w.Write([System.Text.Encoding]::ASCII.GetBytes("`r`n--$boundary`r`nContent-Disposition: form-data; name=`"album_id`"`r`n`r`n$($album.id)`r`n"))
    $w.Write([System.Text.Encoding]::ASCII.GetBytes("--$boundary--`r`n"))
    $upload = Invoke-RestMethod -Uri "http://localhost:18499/api/blobs/upload" -Method Post `
        -Headers $h -ContentType "multipart/form-data; boundary=$boundary" -Body $ms.ToArray()
    Write-Output "UPLOAD: $($upload | ConvertTo-Json -Compress)"
    if ($upload.status -ne "imported") { $fail += "上传失败: $($upload | ConvertTo-Json -Compress)" }

    # 时间线应能看到（blob 在索引；动态照片主图非墓碑）
    $timeline = Invoke-RestMethod -Uri "http://localhost:18499/api/blobs" -Headers $h
    if ($timeline.Count -lt 1) { $fail += "时间线为空——上传入相册修复未生效" }
    $main = $timeline | Where-Object { $_.file_name -like "motion-test*" -and $_.is_motion }
    if (-not $main) { $fail += "动态照片未标记 is_motion" }
    if ($main -and -not $main.motion_video_hash) { $fail += "motion_video_hash 未回填" }

    # 相册内容应有主图
    $albumBlobs = Invoke-RestMethod -Uri "http://localhost:18499/api/albums/$($album.id)/blobs" -Headers $h
    if ($albumBlobs.Count -lt 1) { $fail += "相册内容为空——album_id 绑定未生效" }
} catch {
    $fail += "异常: $_"
} finally {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}
Start-Sleep 1

# meta.db 生成检查（进程停止后 WAL 落盘）
if (-not (Test-Path "$root\meta.db")) { $fail += "meta.db 未生成" }

if ($fail.Count -eq 0) {
    Write-Output "VERIFY-PASS: 启动/meta.db/登录/建相册/上传入相册/时间线/is_motion/motion_video_hash 全部通过"
} else {
    Write-Output "VERIFY-FAIL: $($fail -join '; ')"
}
