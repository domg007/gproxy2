---
title: 部署
description: GPROXY 部署说明：本地（二进制、Docker）与云端（Zeabur）。
---

## 本地部署

### 二进制

1. 从 [GitHub Releases](https://github.com/LeenHawk/gproxy/releases) 下载对应平台二进制。
2. 准备配置文件：

```bash
cp gproxy.example.toml gproxy.toml
```

3. 启动服务：

```bash
./gproxy
```

启动后可访问：

- 管理端：`http://127.0.0.1:8787/`

### Docker

构建镜像：

```bash
docker build -t gproxy:local .
```

运行容器：

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite:///app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  gproxy:local
```

## 云端部署

### Zeabur

当前云端模板仅提供 Zeabur。

- 模板文件：[`zeabur.yaml`](https://github.com/LeenHawk/gproxy/blob/main/zeabur.yaml)
- 预构建镜像：`ghcr.io/leenhawk/gproxy:latest`

推荐配置：

- `GPROXY_ADMIN_KEY`：必填
- `GPROXY_HOST`：`0.0.0.0`
- `GPROXY_PORT`：`8787`
- `GPROXY_DATA_DIR`：`/app/data`
- 将 `/app/data` 挂载为持久化卷

可选配置：

- `GPROXY_DSN`（外部数据库或自定义 sqlite 路径）
- `GPROXY_PROXY`（上游代理）
- `RUST_LOG`（日志级别）
