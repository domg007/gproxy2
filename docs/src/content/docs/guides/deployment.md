---
title: Deployment
description: Deploy GPROXY locally (binary, Docker) and in cloud (Zeabur).
---

## Local deployment

### Binary

1. Download the release binary from [GitHub Releases](https://github.com/LeenHawk/gproxy/releases).
2. Prepare config file:

```bash
cp gproxy.example.toml gproxy.toml
```

3. Start service:

```bash
./gproxy
```

After startup, open:

- Admin UI: `http://127.0.0.1:8787/`

### Docker

Build image:

```bash
docker build -t gproxy:local .
```

Run container:

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite://app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  gproxy:local
```

## Cloud deployment

### Zeabur

Current cloud template support is Zeabur.

- Template file: [`zeabur.yaml`](https://github.com/LeenHawk/gproxy/blob/main/zeabur.yaml)
- Prebuilt image: `ghcr.io/leenhawk/gproxy:latest`

Recommended settings:

- `GPROXY_ADMIN_KEY`: required
- `GPROXY_HOST`: `0.0.0.0`
- `GPROXY_PORT`: `8787`
- `GPROXY_DATA_DIR`: `/app/data`
- Persist volume at `/app/data`

Optional:

- `GPROXY_DSN` (external database or custom sqlite path)
- `GPROXY_PROXY` (upstream egress proxy)
- `RUST_LOG` (log level)
