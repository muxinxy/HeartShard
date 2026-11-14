# HeartShard 牧心后台镜像（多架构，预编译二进制注入）
#
# CI 产出的 linux/amd64、linux/arm64、linux/arm/v7 二进制按 TARGETPLATFORM
# 目录布局放入 docker/bin/，buildx 一次构建推送三架构镜像。
# （构建逻辑见 .github/workflows/build.yml 的 docker job）
#
# 运行：
#   docker run -d -p 8420:8420 \
#     -v heartshard-lib:/data/library \
#     -v heartshard-meta:/data/meta \
#     -e HEARTSHARD_LIB_PW=你的相册库口令 \
#     ghcr.io/muxinxy/heartshard-server:latest

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# TARGETPLATFORM 由 buildx 注入：linux/amd64、linux/arm64、linux/arm/v7
ARG TARGETPLATFORM
COPY docker/bin/$TARGETPLATFORM/heartshard-server /usr/local/bin/heartshard-server
COPY docker/bin/server.toml.example /data/meta/server.toml.example

# 数据卷：相册库 + 元数据库
VOLUME ["/data/library", "/data/meta"]
EXPOSE 8420

# 口令两种方式（二选一）：
#   -e HEARTSHARD_LIB_PW=...        环境变量
#   docker run -it …                交互输入（无 TTY 时启动失败）
ENV HEARTSHARD_LIB_PW=""
ENTRYPOINT ["/usr/local/bin/heartshard-server", "--config", "/data/meta/server.toml"]
