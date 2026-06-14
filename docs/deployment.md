# Console 部署形态

gproxy v2 的管理/门户前端(`console/`)是一份静态 SPA(basepath `/console`),可按四种形态分发。
后端 API 面:`/admin/*`(管理区,require admin)、`/user/*`(门户,require session)、网关
`/v1/*`·`/{provider}/v1/*`、ops `/healthz`·`/version`·`/metrics`。

## 构建

```bash
cd console
pnpm install --frozen-lockfile
pnpm build      # tsc -b && vite build && node scripts/sync-to-embed.mjs
```

`pnpm build` 产出 `console/dist/`(版本号取自 `../Cargo.toml`,commit 取自 `git rev-parse`),并
`sync-to-embed.mjs` 把 `dist/` 同步到 `../assets/console/`(供 rust-embed;该目录已 gitignore)。

---

## 形态 0 — 嵌入(默认,rust-embed)

`pnpm build` 后,`cargo build --features full` 把 `assets/console/` 编进二进制;运行 native 实例即
在 `/console` 提供 SPA(`src/http/server/console.rs`,无扩展名路径 SPA fallback 到 `index.html`)。
**SPA 与 API 同源** → cookie / 同源 CSRF 天然工作,无需任何额外配置。单文件分发,首选。

> 未构建前端时 `assets/console/` 只有 `.gitkeep`,`/console` 返回占位文本,不影响 `cargo build`。

## 形态 1 — 独立静态产物 · 同源反代

把 `dist/` 托管到任意静态主机(Nginx / S3+CDN / …),并由反代让**静态资源与 API 同域**:
`/admin`、`/user`、`/healthz`、`/version`、`/metrics`(以及网关路径)反代到 gproxy 实例。

- SPA 在 `/console`(Vite `base: '/console/'`);若要挂在站点根,需改 `vite.config.ts` 的 `base` 重新构建。
- 同源 → cookie / CSRF 直接可用,**无需 CORS**。

## 形态 2 — 独立静态产物 · 跨域直连(依赖 B2)

console 托管在 A 域、API 在 B 域。需要后端 **B2 CORS**:

```bash
# gproxy 实例
GPROXY_CORS_ORIGINS=https://console.example.com   # 逗号分隔多个;必须是完整 scheme://host[:port]
```

启用后:
- CorsLayer 对白名单 origin 放行(凭证式 CORS,显式 origin,绝不 `*`);预检 OPTIONS 自动应答。
- 会话 cookie 自动切到 **`SameSite=None; Secure`**(跨站 XHR 才会携带)→ **两端都必须 HTTPS**。
- 同源 CSRF 检查对白名单 origin(**scheme 精确匹配**,`http://` 不满足 `https://` 白名单项)放行。
- 前端 `api()` 默认 `credentials:"include"`,无需改动。

> ⚠️ 安全:仅把可信的、HTTPS 的 console 源加入 `GPROXY_CORS_ORIGINS`。详见提交 `b33d41a` 的安全评审。

## 形态 3 — edge 平台同域静态资源

把 `dist/` 作为 edge 平台静态资源(Cloudflare Workers assets / Vercel / Netlify / **EdgeOne Pages**)
随 worker 一起部署,与 edge worker **同源** → cookie/CSRF 天然工作,不依赖 CORS。

> **edge 管理面(F8/B6,已完成)**:edge(wasm)worker 现经 `src/http/admin_api/` 跨目标 dispatcher
> **服务完整 `/admin/*` 与 `/user/*`**(CRUD / authz / 可观测 / auth 登录登出 / 特殊 CRUD
> credentials·user-keys·users / 门户 /user/* / OAuth·device login-flows),认证/会话/口令/密封/审计
> 全在 edge 跑。**三项显式降级(返回 501 `not_implemented`)**:`/admin/update/*`(自更新,native-only)、
> `/admin/login-flows/cookie`(claudecode cookie 登录需 wreq 浏览器 TLS)、`/admin/credentials/{id}/usage`
> (上游实时用量需 wreq)。这三项需 native 实例。其余纯 edge 部署即可提供完整管理区/门户(SPA 由平台
> 静态资源托管,同源 cookie/CSRF/审计 天然工作)。edge 与 native 可共享同一 Turso + Upstash 后端。

---

## 开发模式

```bash
cd console && pnpm dev    # Vite dev server
# 后端:GPROXY_INSECURE_COOKIES=1 cargo run --features full   (本地明文 HTTP)
```

Vite dev server 把 `/admin`、`/user`、`/healthz`、`/version`、`/metrics` 代理到 `http://127.0.0.1:8787`
(同源,cookie 直接可用)。

## 部署形态 × 依赖速查

| 形态 | 同源? | 需 B2 CORS | 需 HTTPS | 管理区/门户可用 |
|---|---|---|---|---|
| 0 嵌入(默认) | 是 | 否 | 否(建议) | ✅ |
| 1 静态 · 同源反代 | 是 | 否 | 建议 | ✅ |
| 2 静态 · 跨域直连 | 否 | ✅ | ✅(SameSite=None) | ✅ |
| 3 edge 平台同域 | 是 | 否 | 建议 | ✅ 完整(除 self-update / cookie 登录 / cred-usage 三项 501 降级) |
