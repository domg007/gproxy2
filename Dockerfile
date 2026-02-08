# syntax=docker/dockerfile:1

FROM node:lts-alpine3.23 AS frontend

WORKDIR /app

COPY apps/gproxy/frontend/package.json apps/gproxy/frontend/pnpm-lock.yaml ./apps/gproxy/frontend/
RUN corepack enable \
    && cd apps/gproxy/frontend \
    && pnpm install --frozen-lockfile

COPY apps/gproxy/frontend ./apps/gproxy/frontend
RUN cd apps/gproxy/frontend && pnpm build

FROM rust:1.92-slim-trixie AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        git \
        pkg-config \
        libssl-dev \
        ca-certificates \
        cmake \
        ninja-build \
        perl \
        upx-ucl \
        libclang-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY route.md ./route.md

COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

RUN cargo build --release -p gproxy \
    && upx --best --lzma target/release/gproxy

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gproxy /usr/local/bin/gproxy

ENV GPROXY_HOST=0.0.0.0
ENV GPROXY_PORT=8787
ENV GPROXY_DATA_DIR=/app/data

EXPOSE 8787

CMD ["/bin/sh", "-c", "set -eu; host=\"${GPROXY_HOST:-0.0.0.0}\"; port=\"${GPROXY_PORT:-8787}\"; data_dir=\"${GPROXY_DATA_DIR:-/app/data}\"; admin_key=\"${GPROXY_ADMIN_KEY:-pwd}\"; proxy=\"${GPROXY_PROXY:-}\"; case \"$host\" in '${GPROXY_HOST}'|'') host='0.0.0.0' ;; esac; case \"$port\" in '${GPROXY_PORT}'|'') port='8787' ;; esac; case \"$data_dir\" in '${GPROXY_DATA_DIR}'|'') data_dir='/app/data' ;; esac; case \"$admin_key\" in '${GPROXY_ADMIN_KEY}'|'') admin_key='pwd' ;; esac; dsn=\"${GPROXY_DSN:-sqlite://${data_dir}/db/gproxy.db?mode=rwc}\"; case \"$dsn\" in '${GPROXY_DSN}') dsn='' ;; esac; case \"$proxy\" in '${GPROXY_PROXY}') proxy='' ;; esac; set -- /usr/local/bin/gproxy --host \"$host\" --port \"$port\" --admin-key \"$admin_key\"; [ -n \"$dsn\" ] && set -- \"$@\" --dsn \"$dsn\"; [ -n \"$proxy\" ] && set -- \"$@\" --proxy \"$proxy\"; exec \"$@\""]
