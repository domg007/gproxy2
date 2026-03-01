# syntax=docker/dockerfile:1.7

FROM node:lts-alpine AS frontend

WORKDIR /app

COPY Cargo.toml ./Cargo.toml
COPY apps/gproxy/frontend/package.json apps/gproxy/frontend/pnpm-lock.yaml ./apps/gproxy/frontend/
RUN corepack enable \
    && cd apps/gproxy/frontend \
    && pnpm install --frozen-lockfile

COPY apps/gproxy/frontend ./apps/gproxy/frontend
RUN cd apps/gproxy/frontend && pnpm build

FROM --platform=$TARGETPLATFORM rust:latest AS builder

WORKDIR /app

ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        file \
        git \
        libclang-dev \
        musl-tools \
        ninja-build \
        perl \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

RUN ARCH="${TARGETARCH:-$(uname -m)}" \
    && case "${ARCH}" in \
      amd64|x86_64) RUST_TARGET="x86_64-unknown-linux-musl" ;; \
      arm64|aarch64) RUST_TARGET="aarch64-unknown-linux-musl" ;; \
      *) echo "unsupported arch: ${ARCH}" >&2; exit 1 ;; \
    esac \
    && rustup target add "${RUST_TARGET}" \
    && libclang_so="$(find /usr/lib -name 'libclang.so*' | head -n 1)" \
    && test -n "${libclang_so}" \
    && LIBCLANG_PATH="$(dirname "${libclang_so}")" cargo build --release --target "${RUST_TARGET}" -p gproxy \
    && mkdir -p /tmp/app/data \
    && cp "target/${RUST_TARGET}/release/gproxy" /tmp/gproxy \
    && file /tmp/gproxy | grep -q "statically linked"

FROM gcr.io/distroless/static

WORKDIR /app

COPY --from=builder /tmp/gproxy /usr/local/bin/gproxy
COPY --from=builder /tmp/app/data /app/data

ENV GPROXY_HOST=0.0.0.0
ENV GPROXY_PORT=8787
ENV GPROXY_DATA_DIR=/app/data
ENV GPROXY_DSN=sqlite://app/data/gproxy.db?mode=rwc

EXPOSE 8787

ENTRYPOINT ["/usr/local/bin/gproxy"]
