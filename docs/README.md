# GPROXY docs

本目录是 `GPROXY` 的文档站点，使用 `Astro + Starlight` 构建。

## 本地开发

```bash
cd docs
pnpm install
pnpm dev
```

默认地址：`http://localhost:4321`

## 构建

```bash
cd docs
pnpm build
pnpm preview
```

## 目录约定

- `src/content/docs/`：文档页面（`.md` / `.mdx`）
- `public/`：静态资源（可通过 `/xxx` 访问）
- `astro.config.mjs`：站点标题、侧边栏、社交链接
