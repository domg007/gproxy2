# syntax=docker/dockerfile:1.7

ARG TARGETARCH

FROM node:lts-alpine AS frontend

WORKDIR /app

COPY Cargo.toml ./Cargo.toml
COPY apps/gproxy/frontend/package.json apps/gproxy/frontend/pnpm-lock.yaml ./apps/gproxy/frontend/
RUN corepack enable \
    && cd apps/gproxy/frontend \
    && pnpm install --frozen-lockfile

COPY apps/gproxy/frontend ./apps/gproxy/frontend
RUN cd apps/gproxy/frontend && pnpm build

FROM --platform=$BUILDPLATFORM ghcr.io/cross-rs/x86_64-unknown-linux-musl:latest AS builder-amd64

WORKDIR /app

RUN if command -v apt-get >/dev/null 2>&1; then \
      apt-get update && apt-get install -y --no-install-recommends \
        clang \
        cmake \
        file \
        libclang-dev \
        ninja-build \
        perl \
        pkg-config \
      && rm -rf /var/lib/apt/lists/*; \
    elif command -v apk >/dev/null 2>&1; then \
      apk add --no-cache \
        clang \
        clang-dev \
        cmake \
        file \
        ninja-build \
        perl \
        pkgconf; \
    else \
      echo "unsupported package manager in builder-amd64" >&2; \
      exit 1; \
    fi

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

RUN libclang_so="$(find /usr -name 'libclang.so*' | head -n 1)" \
    && test -n "${libclang_so}" \
    && ln -sf "${libclang_so}" /usr/lib/libclang.so \
    && LIBCLANG_PATH=/usr/lib cargo build --release --target x86_64-unknown-linux-musl -p gproxy \
    && mkdir -p /tmp/app/data \
    && cp target/x86_64-unknown-linux-musl/release/gproxy /tmp/gproxy \
    && file /tmp/gproxy | grep -q "statically linked"

FROM --platform=$BUILDPLATFORM ghcr.io/cross-rs/aarch64-unknown-linux-musl:latest AS builder-arm64

WORKDIR /app

RUN if command -v apt-get >/dev/null 2>&1; then \
      apt-get update && apt-get install -y --no-install-recommends \
        clang \
        cmake \
        file \
        libclang-dev \
        ninja-build \
        perl \
        pkg-config \
      && rm -rf /var/lib/apt/lists/*; \
    elif command -v apk >/dev/null 2>&1; then \
      apk add --no-cache \
        clang \
        clang-dev \
        cmake \
        file \
        ninja-build \
        perl \
        pkgconf; \
    else \
      echo "unsupported package manager in builder-arm64" >&2; \
      exit 1; \
    fi

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

RUN libclang_so="$(find /usr -name 'libclang.so*' | head -n 1)" \
    && test -n "${libclang_so}" \
    && ln -sf "${libclang_so}" /usr/lib/libclang.so \
    && LIBCLANG_PATH=/usr/lib cargo build --release --target aarch64-unknown-linux-musl -p gproxy \
    && mkdir -p /tmp/app/data \
    && cp target/aarch64-unknown-linux-musl/release/gproxy /tmp/gproxy \
    && file /tmp/gproxy | grep -q "statically linked"

ARG TARGETARCH
FROM builder-${TARGETARCH} AS builder

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
