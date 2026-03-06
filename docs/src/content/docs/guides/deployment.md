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

Pull prebuilt image (recommended):

```bash
docker pull ghcr.io/leenhawk/gproxy:latest
```

Run container:

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite:///app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  ghcr.io/leenhawk/gproxy:latest
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

### Release downloads and self-update (Cloudflare Pages)

- The release workflow also deploys a dedicated Cloudflare Pages downloads site for binaries and update manifests.
- Default public base URL: `https://download-gproxy.leenhawk.com`
- Generated manifests:
  - `/manifest.json` — full file index for the docs downloads page
  - `/releases/manifest.json` — stable self-update channel
  - `/staging/manifest.json` — staging self-update channel
- The admin UI `Cloudflare` update source reads from this site.
- Required repository secrets for the downloads deployment:
  - `CLOUDFLARE_API_TOKEN`
  - `CLOUDFLARE_ACCOUNT_ID`
  - `CLOUDFLARE_DOWNLOADS_PROJECT_NAME`
- Optional repository secrets:
  - `DOWNLOAD_PUBLIC_BASE_URL`
  - `UPDATE_SIGNING_KEY_ID`
  - `UPDATE_SIGNING_PRIVATE_KEY_B64`
  - `UPDATE_SIGNING_PUBLIC_KEY_B64`
