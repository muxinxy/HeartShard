# HeartShard 牧心后台（多阶段构建）
# 构建镜像内完成 Rust 编译 + 前端资产打包；运行镜像仅含二进制。
#
# 构建： docker build -t heartshard-server .
# 运行： docker run -d -p 8420:8420 \
#          -v heartshard-lib:/data/library \
#          -v heartshard-meta:/data/meta \
#          -e HEARTSHARD_LIB_PW=你的相册库口令 \
#          heartshard-server
#
# 或用 docker-compose（见仓库 docker-compose.yml）。

FROM rust:1.85-slim AS builder
WORKDIR /build

# 构建依赖（pkg-config + sqlite3 头文件）
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libsqlite3-dev nodejs \
    && rm -rf /var/lib/apt/lists/*

# 先拷贝清单以利用层缓存
COPY core-rs/Cargo.toml core-rs/Cargo.lock core-rs/
COPY core-rs/crates core-rs/crates
COPY tools/syncer tools/syncer
COPY apps apps
COPY patches patches
COPY server server

WORKDIR /build/server
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/server/target/release/heartshard-server /usr/local/bin/heartshard-server

# 数据卷：相册库 + 元数据库
VOLUME ["/data/library", "/data/meta"]
EXPOSE 8420

# 口令两种方式（二选一）：
#   -e HEARTSHARD_LIB_PW=...        环境变量
#   docker run -it …                交互输入（无 TTY 时启动失败）
ENV HEARTSHARD_LIB_PW=""
ENTRYPOINT ["/usr/local/bin/heartshard-server", "--config", "/data/meta/server.toml"]
