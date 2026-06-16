# Agent CLI 请求头采集(模型路径) + 动态头清单

> 用途:为 gproxy 各渠道**请求头伪装**(不止 UA)提供真实 CLI 的完整头集合。
> 采集日期:2026-06-12 · 脚本:[`scripts/capture_headers.py`](../scripts/capture_headers.py)(转发型 MITM,dump 全部有序请求头)。
> **所有 `Authorization`/`Bearer`/AWS SigV4/`x-amz-security-token` 值已脱敏。**
> 标注:**[静态]** 可直接注入;**[动态]** 每请求/每会话变化,需反编译生成逻辑;**[凭证]** 鉴权,非伪装;**[传输]** http 客户端自动加(Host/Content-Length/Accept-Encoding/Connection)。

---

## claudecode — `POST api.anthropic.com/v1/messages?beta=true`

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `anthropic-version` | `2023-06-01` | 静态 |
| `anthropic-dangerous-direct-browser-access` | `true` | 静态 |
| `x-app` | `cli` | 静态 |
| `anthropic-beta` | `claude-code-20250219,context-1m-2025-08-07,interleaved-thinking-2025-05-14,thinking-token-count-2026-05-13,context-management-2025-06-27,prompt-caching-scope-2026-01-05,mid-conversation-system-2026-04-07,advisor-tool-2026-03-01,effort-2025-11-24`(默认集 + 转发入站的 beta) | 静态(+合并) |
| `X-Stainless-Lang` | `js` | 静态 |
| `X-Stainless-Runtime` | `node` | 静态 |
| `X-Stainless-Runtime-Version` | `v24.3.0`(node 版本) | 半静态 |
| `X-Stainless-Package-Version` | `0.94.0`(`@anthropic-ai/sdk` 版本;**会随 SDK 升级漂移**,cpa 抄的是旧的 0.74.0) | 半静态 |
| `X-Stainless-OS` | `Linux`(平台:MacOS/Windows/Linux) | 平台 |
| `X-Stainless-Arch` | `x64`(平台:x64/arm64) | 平台 |
| `X-Stainless-Timeout` | `600` | 静态 |
| `Accept` / `Content-Type` | `application/json` | 静态 |
| `User-Agent` | `claude-cli/2.1.162 (external, cli)` | 静态 |
| **`X-Stainless-Retry-Count`** | `0`,重试递增 | **动态(计数器)** |
| **`X-Claude-Code-Session-Id`** | UUIDv4,**每会话**(会话内稳定) | **动态** |
| **`x-client-request-id`** | UUID,**每请求**,**仅 `api.anthropic.com` 加**(走代理则无) | **动态** ⚠️待反编译 |
| `Authorization` | `Bearer …` | 凭证 |

> gproxy 现有 claudecode 已注入大部分(x-stainless-* / x-app / anthropic-beta / session-id)。需核对:`X-Stainless-Package-Version` 是否跟到 0.94.0、`anthropic-beta` 默认集是否最新。

## codex — `POST chatgpt.com/backend-api/codex/responses`(h2)

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `accept` | `text/event-stream` | 静态 |
| `content-type` | `application/json` | 静态 |
| `originator` | `codex_exec` | 静态 |
| `user-agent` | `codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color (codex_exec; 0.137.0)` | 静态 |
| `x-codex-beta-features` | `terminal_resize_reflow,memories` | 静态 |
| **`session-id`** | UUIDv7 | **动态** |
| **`thread-id`** | UUIDv7(本次 = session-id 同值) | **动态** |
| **`x-client-request-id`** | UUIDv7(本次 = session-id 同值) | **动态** ⚠️ |
| **`x-codex-window-id`** | `<session-uuidv7>:0` | **动态** |
| **`x-codex-turn-metadata`** | JSON:`{session_id, thread_id, thread_source:"user", turn_id:<另一UUIDv7>, workspaces:{<cwd>:{latest_git_commit_hash, has_changes}}, sandbox:"seccomp", turn_started_at_unix_ms:<ms时间戳>, request_kind:"turn", window_id}` | **动态(最复杂)** ⚠️⚠️ |
| `authorization` | `Bearer …` | 凭证 |

> ⚠️ **codex 最难**:`session-id/thread-id/window-id/x-client-request-id` 共享**一个会话级 UUIDv7**,`turn_id` 是**每轮新 UUIDv7**;`x-codex-turn-metadata` 还内嵌 **unix 毫秒时间戳 + 当前目录 git commit hash + has_changes + sandbox**。gproxy 现有 codex 注入了 session/thread/x-client-request-id/originator,但**缺** `x-codex-turn-metadata` / `x-codex-window-id` / `x-codex-beta-features`(codex 新增)。turn-metadata 的 git 字段代理侧无法真实获知,需伪造或省略——**需反编译确认服务端是否校验**。

## geminicli — `POST cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `Content-Type` | `application/json` | 静态 |
| `User-Agent` | `GeminiCLI-tui/0.46.0/<model> (linux; x64; terminal) google-api-nodejs-client/9.15.1` | 静态(模型动态) |
| `x-goog-api-client` | `gl-node/22.20.0`(node 版本) | 半静态 |
| `Accept` | `*/*` | 静态 |
| `Authorization` | `Bearer …` | 凭证 |

> ✅ **干净**:无 UUID/会话动态头。除凭证外都可静态注入。

## antigravity — `POST (daily-)cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `User-Agent` | `antigravity/cli/1.0.6 linux/amd64` | 静态 |
| `Content-Type` | `application/json` | 静态 |
| `Transfer-Encoding` | `chunked`(流式请求体) | 传输 |
| `Accept-Encoding` | `gzip` | 传输 |
| `Authorization` | `Bearer …` | 凭证 |

> ✅ **已抓真实推理调用 `streamGenerateContent` 确认**:除凭证只有 UA + Content-Type(Host/Transfer-Encoding/Accept-Encoding 由 http 客户端加)。**没有 `requestId` / `requestType` / `Accept`** —— 代码原本注入的这三个(从旧版 mined)真实 1.0.6 CLI 不发,已移除。无动态 id 头。

## copilot_cli — `api.individual.githubcopilot.com`(`/models`、`/mcp/readonly`、chat)

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `user-agent` | `copilot/1.0.61 (linux v24.16.0) term/unknown` | 静态 |
| `copilot-integration-id` | `copilot-developer-cli` | 静态 |
| `editor-version` | `copilot/1.0.61` | 静态 |
| `openai-intent` | `conversation-agent` | 静态 |
| `x-github-api-version` | `2026-06-01` | 静态 |
| `x-initiator` | `user` | 静态 |
| `x-mcp-host` | `copilot-cli`(仅 mcp 调用) | 静态 |
| `x-mcp-tools` | `get_file_contents,search_code,…`(仅 mcp 调用) | 静态 |
| `accept` | `application/json`(chat 加 `, text/event-stream`) | 静态 |
| `accept-encoding` | `zstd,gzip,deflate,br` | 静态 |
| **`x-client-machine-id`** | UUIDv4,**每机器持久**(生成一次存本地) | **动态** ⚠️ |
| **`x-interaction-id`** | UUIDv4,**每次交互** | **动态** ⚠️ |
| `authorization` | `Bearer …` | 凭证 |

## kiro — Kiro CLI(`*.kiro.dev`,AWS-JSON 1.0 Smithy,**Bearer** 非 SigV4)

> 2026-06-16 用 mitmproxy 实抓本地 `kiro-cli-chat`(GitHub 登录)。**纠正上一版**:不是老 Amazon Q 的 `q.us-east-1.amazonaws.com` + SigV4,而是 **Kiro 产品**的 `*.kiro.dev` 双 host + AWS-JSON 1.0 Smithy + **Bearer token**。两个服务两个 host:

| 操作 | host / `x-amz-target` |
|---|---|
| 模型列表 | `POST https://management.{region}.kiro.dev/?origin=KIRO_CLI&profileArn=<arn>` · `AmazonCodeWhispererService.ListAvailableModels` |
| 聊天 | `POST https://runtime.{region}.kiro.dev/` · `AmazonCodeWhispererStreamingService.GenerateAssistantResponse` |
| 用量 | `management.{region}.kiro.dev` · `AmazonCodeWhispererService.GetUsageLimits`(未抓,二进制确认) |
| 登录 | `prod.us-east-1.auth.desktop.kiro.dev` |

公共头(两个操作一致):

| 头 | 值 / 形态 | 类 |
|---|---|---|
| `content-type` | `application/x-amz-json-1.0` | 静态 |
| `x-amz-target` | `AmazonCodeWhisperer{Service\|StreamingService}.<Op>` | 静态(按操作) |
| `user-agent` / `x-amz-user-agent` | `aws-sdk-rust/1.3.15 ua/2.1 api/codewhisperer{runtime\|streaming}/0.1.16551 … md/appVersion-2.6.1 app/AmazonQ-For-CLI` | 静态(管理面=`runtime`,运行面=`streaming`) |
| `x-amzn-codewhisperer-optout` | `false` | 静态 |
| `authorization` | `Bearer <kiro.dev token>`(**非 SigV4,无 `x-amz-date`/`x-amz-security-token`**) | 凭证 |
| **`amz-sdk-invocation-id`** | UUIDv4 / 每请求 | 动态(AWS SDK 自带) |
| `amz-sdk-request` | `attempt=1; max=3` | 静态 |
| `accept` / `accept-encoding` | `*/*` / `gzip` | 静态 |

> ⚠️ **没有 `x-amzn-kiro-agent-mode`**(真实请求不带,v2 之前误加);`origin` = **`KIRO_CLI`**(不是 v1 的 `AI_EDITOR`)。

模型列表 body / resp:
```
REQ : {"origin":"KIRO_CLI","profileArn":"arn:aws:codewhisperer:us-east-1:<acct>:profile/<id>"}
RESP: {"defaultModel":{"modelId":"auto"},"models":[{"modelId":"claude-sonnet-4.5","modelName":…,"tokenLimits":…},…]}
真实模型:auto / claude-sonnet-4.5 / claude-sonnet-4 / claude-haiku-4.5 / deepseek-3.2 / minimax-m2.5 / minimax-m2.1 / glm-5 / qwen3-coder-next
```
聊天:body = `{"conversationState":{conversationId,history,currentMessage,…}}`;resp = AWS 事件流(`assistantResponseEvent` 等帧)。

**登录 = 四种凭证 kind**(2026-06-16 反编译 `kiro-cli`/`fig_auth` 实证,枚举字面量 `social | idc | builderId | external_idp`)。`kiro-cli login` 菜单实为 `Use with Google / GitHub / Builder ID / Your Organization`(后者 = idc);**external_idp 不是菜单项**(见下 (d))。共同点:成功后凭证 `access_token` 注入 `Authorization: Bearer`(60s 提前刷新 skew);凭证存 SQLite `auth_kv(key,value)`,**Linux 无 keyring**。

**(a) social — GitHub / Google**(device-code,实抓):
```
① POST prod.{region}.auth.desktop.kiro.dev/oauth/device/authorization
   body {"clientId":"Kiro-CLI","loginProvider":"Github"}  (或 "Google")
   resp {deviceCode,userCode,verificationUriComplete:"https://app.kiro.dev/account/device?user_code=…&login_provider=Github",
         intervalInMilliseconds:5000, expiresInMilliseconds:300000}
② POST …/oauth/device/poll  {"deviceCode","clientId":"Kiro-CLI"}
   pending {"status":"authorization_pending"} → done {"status":"authorized", accessToken, refreshToken, profileArn, identityProvider}
③ 刷新 POST …/refreshToken  {"refreshToken": rt}  (无 clientId/secret)
凭证存 {access_token, refresh_token, profile_arn, provider}。
```

**(b) builderId / (c) idc — AWS SSO-OIDC**(authcode+PKCE,host `https://oidc.{region}.amazonaws.com`,REST-JSON,**无 x-amz-target / 无 SigV4**)。两者同一套代码,仅 `start_url`+`region` 不同(builderId=`view.awsapps.com/start`+`us-east-1`;idc=自配 `auth.idc.start-url`+`auth.idc.region`):
```
① RegisterClient POST /client/register
   {clientName:"Kiro-CLI", clientType:"public", scopes:[codewhisperer:completions/analysis/conversations],
    grantTypes:["authorization_code","refresh_token"], redirectUris:[loopback], issuerUrl:start_url}
   → {clientId, clientSecret}
② 浏览器 GET /authorize?response_type=code&client_id&redirect_uri&scopes=<空格join,plural!>&state&code_challenge&code_challenge_method=S256
③ CreateToken POST /token  {grantType:"authorization_code", clientId, clientSecret, code, redirectUri, codeVerifier} → {accessToken, refreshToken, expiresIn}
刷新 POST /token {grantType:"refresh_token", clientId, clientSecret, refreshToken}
凭证存 {access_token, refresh_token, expires_at_ms, client_id, client_secret, region, start_url}。
```
> ⚠️ `scopes`(**plural**,空格 join)是 SSO-OIDC `/authorize` 的非标准约定;PKCE 默认流,device-code 仅为浏览器打不开时的回退。`scopePrefix` 设置默认 `codewhisperer`。**`kirocli:odic:token` 等是 SQLite KEY,不是 scope。**

**(d) external_idp — 运营方自配 OIDC**(authcode+PKCE,public client,**无动态注册/无 client_secret**)。⚠️ **不是 `kiro-cli login` 的菜单项**——它由组织的"统一登录门户"(app.kiro.dev)回调里带回的 IdP 元数据自动触发(二进制:`Using unified auth portal for login`、`External IdP metadata extracted from callback`),菜单只有上面三项 + "Use with Your Organization"(=idc)。**v2 不实现**,仅留此 RE 记录:
```
① discovery GET {issuer_url}/.well-known/openid-configuration → {authorization_endpoint, token_endpoint}
② 浏览器 GET {authorization_endpoint}?response_type=code&client_id&redirect_uri&scope=<configured+offline_access>&state&code_challenge&code_challenge_method=S256
③ 交换 POST {token_endpoint}  (application/x-www-form-urlencoded)  grant_type=authorization_code&code&redirect_uri&code_verifier&client_id → {access_token, refresh_token, expires_in}
刷新 POST {token_endpoint}  grant_type=refresh_token&refresh_token&client_id
```

> **v2 现状**:模型列表(`76162e0`)+ 聊天/用量(`11b804b`)走 `*.kiro.dev` Smithy。**菜单的三种登录已实现**:social GitHub/Google 走 `device_start`(`params.login_provider`);builderId/idc 走 `authcode_start`/`authcode_exchange`(`params.auth_method`,PKCE-only);两路 `refresh` 按凭证形态分派(`client_id`+`client_secret`→SSO-OIDC;否则 social)。代码:`src/channel/bulletins/kiro/auth/{social,sso_oidc}.rs`。**external_idp 故意不做**(非菜单项,门户驱动)。Console 向导四项对齐 kiro-cli 菜单(GitHub/Google/Builder ID/Your Organization)。**E2E 待真实账号验证。**

---

## 需反编译的动态头(汇总,给反编译同学)

| 渠道 | 头 | 形态 | 关联/难点 |
|---|---|---|---|
| claudecode | `X-Claude-Code-Session-Id` | UUIDv4 / 每会话 | 简单 |
| claudecode | `x-client-request-id` | UUID / 每请求(仅 api.anthropic.com) | 简单 |
| **codex** | `session-id`/`thread-id`/`x-client-request-id`/`x-codex-window-id` | **UUIDv7,会话级同源** | window-id 末尾 `:0` |
| **codex** | `x-codex-turn-metadata` | **JSON,内嵌 turn UUIDv7 + unix-ms + git commit + has_changes + sandbox** | **最复杂,需确认服务端校验强度** |
| copilot | `x-client-machine-id` | UUIDv4 / 每机器持久 | 存本地复用 |
| copilot | `x-interaction-id` | UUIDv4 / 每交互 | 简单 |
| kiro | `amz-sdk-invocation-id` | UUIDv4 / 每请求 | AWS SDK 自带(Bearer,**无** `x-amz-date`/SigV4) |

**最关键反编译目标:codex 的 `x-codex-turn-metadata`**(JSON 结构 + UUIDv7 + 时间戳 + git 状态)和会话级 UUIDv7 的生成/复用规律。其余(gemini/antigravity)无动态伪装头。

---

## 生成机制(源码 + 反编译确认)

> codex 直接读开源源码 `samples/codex/codex-rs/`;claude/copilot/agy/kiro-cli 反编译四个原生二进制确认。

### codex(源码确认 · `samples/codex/codex-rs/`)
- **session-id / thread-id**:`Uuid::now_v7()`,**会话级**(进程启动时各生成一个 v7;`codex exec` 单发模式下我抓到三者同值)。`protocol/src/session_id.rs:20`、`thread_id.rs:18`。
- **x-client-request-id** = `thread_id`(`codex-api/src/endpoint/responses.rs:92`)。
- **x-codex-window-id** = `<thread_id>:0`,`:0` 是 auto-compact 窗口计数器(`core/src/client.rs:639`)。
- **x-codex-turn-metadata**(`core/src/turn_metadata.rs`):
  - `turn_id` = **每轮新 `Uuid::now_v7()`**;`turn_started_at_unix_ms` = `SystemTime::now()` 毫秒(`core/src/turn_timing.rs:183`)。
  - `workspaces.<cwd>.latest_git_commit_hash` = **`git rev-parse HEAD`**;`has_changes` = **`git status --porcelain` 非空**(`git-utils/src/info.rs:164/281`,异步 enrich)。
  - `sandbox`=权限档标签、`request_kind`=`"turn"`、`window_id` 同上。
- **originator**:`CODEX_INTERNAL_ORIGINATOR_OVERRIDE_ENV_VAR` 覆盖,默认 `codex_cli_rs`(`login/src/auth/default_client.rs:36`)→ **`codex exec` 走 exec 路径把它设成 `codex_exec`**(交互式 TUI 就是默认 `codex_cli_rs`)。
- **user-agent** = `{originator}/{CARGO_PKG_VERSION} ({os_info}; {arch}) {terminal} (suffix)`,suffix 来自 `USER_AGENT_SUFFIX`。
- **x-codex-beta-features**:ModelClient 配置串,会话级常量。
- **gproxy 复刻要点**:生成一个会话级 UUIDv7(复用给 session/thread/window/x-client-request-id)、每轮新 UUIDv7(turn_id)、毫秒时间戳、turn-metadata JSON。**git 字段代理拿不到客户端真实 cwd**——伪造或省略 `workspaces`,需观察后端是否报错(后端闭源,只能试)。

### claude(反编译 `~/.local/bin/claude`,Bun 打包 JS)
- **x-claude-code-session-id** = `crypto.randomUUID()`(**UUIDv4**),**每进程/会话**一个。gproxy 现已生成。
- **x-client-request-id**:`@anthropic-ai/sdk`(Stainless)每请求生成的 UUID,**仅直连 api.anthropic.com 时加**。
- **x-stainless-***:SDK 自动注入;`x-stainless-package-version` = SDK 版本(**抓到 0.94.0,gproxy 代码里是 0.81.0,该升**),`runtime-version` = node 版本(`v24.3.0`),`os`/`arch` = 运行平台,`retry-count` 重试递增。
- 机制简单:**UUIDv4 + SDK 默认头**,无复杂结构。

### copilot(反编译 copilot-linux-x64,Bun 打包 JS)
- **x-client-machine-id** = `crypto.randomUUID()`(**UUIDv4**),**持久化复用**(每机器一次,存本地配置)→ gproxy 按账号/凭证生成一次并复用即可。
- **x-interaction-id** = `crypto.randomUUID()`(**UUIDv4**),**每次交互**。
- 其余(`copilot-integration-id`/`editor-version`/`openai-intent`/`x-github-api-version`/`x-initiator`)全静态。

### agy(反编译 `~/.local/bin/agy`,Go)
- 模型路径(cloudcode-pa)**无任何动态 id 头**;UA `antigravity/cli/1.0.6 linux/amd64` 静态。✅ 无需生成逻辑。

### kiro-cli(2026-06-16 mitmproxy 实抓 `~/.local/bin/kiro-cli-chat`,Rust + AWS SDK)
- **纠正旧结论**:不是 SigV4。`kiro-cli`(Kiro 产品,GitHub 登录)走 `*.kiro.dev` 双 host + AWS-JSON 1.0 Smithy + **Bearer token**——详见上面 kiro 小节。动态头只有 `amz-sdk-invocation-id`(UUIDv4/请求);**无** `x-amz-date`/`x-amz-security-token`/SigV4。**无** `x-amzn-kiro-agent-mode`;`x-amzn-codewhisperer-optout: false`;`origin: KIRO_CLI`。
