# syntax=docker/dockerfile:1

FROM node:lts-alpine AS frontend

WORKDIR /app

COPY apps/gproxy/frontend/package.json apps/gproxy/frontend/pnpm-lock.yaml ./apps/gproxy/frontend/
RUN corepack enable \
    && cd apps/gproxy/frontend \
    && pnpm install --frozen-lockfile

COPY apps/gproxy/frontend ./apps/gproxy/frontend
RUN cd apps/gproxy/frontend && pnpm build

FROM rust:1.92-alpine3.23 AS builder

WORKDIR /app

RUN apk add --no-cache \
        build-base \
        clang \
        cmake \
        file \
        git \
        musl-dev \
        ninja \
        perl \
        pkgconfig

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

COPY --from=frontend /app/apps/gproxy/frontend/dist ./apps/gproxy/frontend/dist

ARG TARGETARCH
RUN case "${TARGETARCH}" in \
      "amd64") export RUST_TARGET="x86_64-unknown-linux-musl" ;; \
      "arm64") export RUST_TARGET="aarch64-unknown-linux-musl" ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && rustup target add "${RUST_TARGET}" \
    && cargo build --release --target "${RUST_TARGET}" -p gproxy \
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
ENV GPROXY_ADMIN_KEY=pwd

EXPOSE 8787

ENTRYPOINT ["/usr/local/bin/gproxy"]
