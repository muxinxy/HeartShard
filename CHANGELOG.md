# 更新日志

HeartShard（牧心相册）的所有重要变更记录于此。
英文版见 [CHANGELOG_EN.md](CHANGELOG_EN.md)。

## [0.1.0] - 2026-11-13

首个公开版本。纯客户端、端到端加密的多云相册系统：照片/视频切片后以
AES-256-GCM 加密、Reed-Solomon 纠删码冗余，分散存储在用户自己的
S3 / WebDAV / OpenList / FTPS 存储上——服务器（可选）与云存储全程零知识。

### 新增

**安全**
- 登录滑动验证码（前端拖拽，阻止机器人提交）
- TOTP 两步验证（RFC 6238，RFC 官方向量测试覆盖）：网页端「头像菜单 →
  两步验证」绑定/关闭，登录时动态要求验证码

**核心**
- 加密分片管线：AES-256-GCM（96-bit tag）逐 chunk 加密，Leopard-RS 纠删码
  n=4/m=2 冗余分散，SHA-256 全程校验
- 主密钥封装：随机 MK + 口令派生 KEK 封装（改密码不重传分片）
- 多设备增量同步：Lamport 时钟 + LWW 合并 + delta 发布
- 云存储适配器：S3（手写 SigV4）、WebDAV、OpenList（令牌/账密）、FTPS

**后台服务**
- 牧心后台：多用户认证（argon2id）+ 相册级 ACL（none/view/upload/manage，
  沿树继承、显式覆盖、用户优先于组）+ 常驻同步引擎 + 数据 API
- 元数据存储：SQLite（默认）或 MySQL，旧 JSON 配置自动迁移
- 网页端管理：用户/用户组/同步源/云存储全部网页端配置；存储连通性探针
- 分享：相册或单张/多选照片分享链接，**默认去除 EXIF/GPS 元数据**，
  可选有效期与分享码；浏览器 WebCrypto 解密，服务器零知识
- PWA 网页客户端：时间线/探索（GPS 地点聚类 + 类型分类）/相册/收藏夹/
  回收站/多选批量操作/右键菜单/灯箱（缩放、旋转、翻转、视频播放）
- 动态照片：Android Motion Photo（JPEG 尾随 MP4）检测与视频段提取；
  Apple Live Photo 文件名配对；双端显示、播放与双文件导出
- 部署：Dockerfile + docker-compose + 多架构 Docker 镜像
  （amd64 / arm64 / armv7）；GitHub Actions 多平台多架构编译
  （linux x86_64/aarch64/armv7/i686、windows x86_64、android
  arm64-v8a/armeabi-v7a/x86_64）

**桌面客户端与安卓**
- Windows 便携单文件 exe（数据目录 exe 同级）+ Android APK
- 双模式：本地相册库（serverless 直连云存储）或连接牧心后台
- 时间线（按天分组 + 年月导航）/ 瀑布流网格 / 相册封面墙 / 收藏夹 /
  回收站（可恢复）/ EXIF 解析 / 地图视图 / 加密视频在线播放（Range 流式）
- 全链路调试日志（[http]/[auth]/[upload]/[meta] 等）

### 修复
- 双模式切换后 `api` 固化到本地 provider 导致「连接后台」误走本地解锁
  （现改为动态 Proxy 分派，切换即时生效）
- 上传 multipart 中 `album_id` 排在文件字段后被丢弃 → 照片全部落入
  「未入相册」（现读完全部字段再绑定）
- 照片/视频右键菜单在 Windows 下无响应（菜单 CSS 缺失 + 闭包字符串化）
- 相册分享/多选后弹窗与勾选条残留页面（route 现统一清理）
- 修复后仍显示「降级」：repair 落点变更未回写 manifest.storage_id

### 变更
- 全部用户可见提示中文化
- UI 遵循「相册优先、技术隐藏」：加密/分片细节折叠，交互对齐主流相册应用

### 已知限制
- HEIC 静态图暂不解码（Live Photo 主图按原文件存取）；人脸/场景 AI 分类未实现
- 视频分享暂不去元数据；`/raw` 视频整段载入内存
- iOS 需 macOS/Xcode；鸿蒙壳需 DevEco（详见 docs/development.md）

[0.1.0]: https://github.com/muxinxy/HeartShard/releases/tag/v0.1.0
