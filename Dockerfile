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

FROM --platform=$TARGETPLATFORM ghcr.io/cross-rs/cross:latest AS builder

WORKDIR /app
ENV PATH=/root/.cargo/bin:/usr/local/cargo/bin:$PATH
ARG RUST_TOOLCHAIN=1.85.1

RUN if command -v apt-get >/dev/null 2>&1; then \
      apt-get update && apt-get install -y --no-install-recommends \
        clang \
        cmake \
        curl \
        file \
        libclang-dev \
        musl-tools \
        ninja-build \
        perl \
        pkg-config \
      && rm -rf /var/lib/apt/lists/*; \
    elif command -v apk >/dev/null 2>&1; then \
      apk add --no-cache \
        clang \
        clang-dev \
        cmake \
        curl \
        file \
        musl-dev \
        musl-tools \
        ninja-build \
        perl \
        pkgconf; \
    else \
      echo "unsupported package manager in builder image" >&2; \
      exit 1; \
    fi

RUN if command -v rustup >/dev/null 2>&1; then \
      rustup toolchain install "${RUST_TOOLCHAIN}" --profile minimal; \
    else \
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}"; \
      . "$HOME/.cargo/env"; \
    fi \
    && rustc +"${RUST_TOOLCHAIN}" --version \
    && cargo +"${RUST_TOOLCHAIN}" --version

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

ARG TARGETARCH=x86_64
RUN case "${TARGETARCH}" in \
      amd64|x86_64) export RUST_TARGET="x86_64-unknown-linux-musl" ;; \
      arm64|aarch64) export RUST_TARGET="aarch64-unknown-linux-musl" ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && rustup target add --toolchain "${RUST_TOOLCHAIN}" "${RUST_TARGET}" \
    && libclang_so="$(find /usr -name 'libclang.so*' | head -n 1)" \
    && test -n "${libclang_so}" \
    && ln -sf "${libclang_so}" /usr/lib/libclang.so \
    && LIBCLANG_PATH=/usr/lib cargo +"${RUST_TOOLCHAIN}" build --release --target "${RUST_TARGET}" -p gproxy \
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
