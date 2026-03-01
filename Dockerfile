# syntax=docker/dockerfile:1.7

FROM node:lts-alpine AS frontend

WORKDIR /app

COPY apps/gproxy/frontend/package.json apps/gproxy/frontend/pnpm-lock.yaml ./apps/gproxy/frontend/
RUN corepack enable \
    && cd apps/gproxy/frontend \
    && pnpm install --frozen-lockfile

COPY apps/gproxy/frontend ./apps/gproxy/frontend
RUN cd apps/gproxy/frontend && pnpm build

FROM --platform=$TARGETPLATFORM rust:1.85-alpine AS builder

WORKDIR /app

RUN apk add --no-cache \
      build-base \
      clang \
      clang-dev \
      cmake \
      file \
      git \
      musl-dev \
      ninja \
      perl \
      pkgconf

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

RUN libclang_so="$(find /usr/lib -name 'libclang.so*' | head -n 1)" \
    && test -n "${libclang_so}" \
    && LIBCLANG_PATH="$(dirname "${libclang_so}")" cargo build --release -p gproxy \
    && mkdir -p /tmp/app/data \
    && cp target/release/gproxy /tmp/gproxy \
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
