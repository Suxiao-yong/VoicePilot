# Task 3 —— HTTP/SSE 网关微服务镜像（契约 §3.5）。
#
# 构建（构建上下文 = 本目录，即 cargo workspace 根）：
#   docker build -t voicepilot:latest .
# 运行：
#   docker run --rm -p 8080:8080 -v voicepilot-data:/data voicepilot:latest
#
# 说明：本任务不执行 docker build（执行环境无 docker），仅保证文件自洽。

# ---------- stage 1: build ----------
FROM rust:1.85-bookworm AS builder

WORKDIR /src

# 只构建网关二进制（-p cli → [[bin]] voicepilot）。
# rusqlite 用 bundled feature（内置 SQLite 源码），builder 里的 gcc 已足够；
# 不构建 crates/ui（Tauri 依赖 webkit/GTK，与网关无关）。
COPY . .
RUN cargo build --release -p cli

# ---------- stage 2: runtime ----------
FROM debian:bookworm-slim AS runtime

# rusqlite(bundled) 与 rustls 均为静态/内置实现，运行期只需 CA 证书
# （LLM HTTP 走 VOICEPILOT_LLM_BASE_URL，需要校验服务端证书）。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/voicepilot /usr/local/bin/voicepilot

# SQLite 库（含 WAL）落在挂载卷里。
RUN mkdir -p /data
VOLUME ["/data"]

EXPOSE 8080

# 启动会打印 `voicepilot serve listening on http://0.0.0.0:8080`（benchmark 解析这一行）。
ENTRYPOINT ["voicepilot", "serve", "--host", "0.0.0.0", "--port", "8080", "--db", "/data/voicepilot.db"]
