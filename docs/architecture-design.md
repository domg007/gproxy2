# gproxy v2 架构重构设计

> 状态:已与项目作者对齐,待评审落地。
> 日期:2026-06-01
> 范围:从零重构(near-greenfield),目标是去臃肿、补齐跨模型负载均衡与多实例支持。

## 1. 背景与动机

v1 在 "vibe coding" 阶段长出,架构边界不清,导致两类问题:

1. **维护性差 / 臃肿**。真实代码约 110K 行,臃肿集中在三处:
   - `sdk/gproxy-protocol` 转换层(52K 行):`transform/<from>/.../<to>/...` 目录平方级爆炸,每对协议手写 request/response/stream。
   - `crates/gproxy-api/src/provider/handler.rs`(4333 行)上帝文件。
   - `sdk/gproxy-engine/src/engine.rs`(2770 行):`execute_inner` / `execute_stream_inner` 巨函数。
2. **关键能力做不了**。当前架构没有 "逻辑模型 → 多后端" 的路由层,也没有可共享的运行时状态后端(`RateLimitBackend` / `QuotaBackend` / `AffinityBackend` 仅内存实现),因此**跨模型负载均衡**和**多实例**都无法落地。

v2 是一次全量重写,但**不是字面意义的从零**:协议转换与 channel 的"逻辑知识"尽量移植,组织方式彻底重做。

## 2. 已锁定的核心决策

| # | 决策 | 理由 |
|---|---|---|
| 1 | **单编译单元**:一个 crate(lib + bin),按模块划分。砍掉多 crate / SDK 发布架构(lock-step 版本、path+version 双声明、发布 DAG、`gproxy-sdk` 门面)。 | 不再以发布 crates.io SDK 为目标,组织自由度最大化。 |
| 2 | 协议转换**保留两两转换(保真度优先)**,用统一 trait + 共享样板(`transform/common/`)收敛重复代码。**不引入会丢字段的 IR**。 | 两两转换是跨协议互转的核心资产,IR 会丢 provider 特有字段。 |
| 3 | **两层负载均衡**:逻辑路由 → 后端池(member 间均衡);后端内 → 凭证池轮转。 | v1 缺失的核心能力。 |
| 4 | **分层存储**:单实例内存缓存 / 多实例 Redis 缓存 / DB 始终是持久化真相源。**每个数据域自声明策略**(只缓存 / 只持久化 / 写穿)。 | 高频计数走缓存,钱与配置强持久化,日志只落库,登录态只缓存。 |
| 5 | **Redis 可选**:单实例零外部依赖(memory 后端),多实例才需 Redis。 | 单机用户开箱即用。 |
| 6 | **保留 SeaORM,实体全部重新设计**。 | 实体/迁移知识可移植,但 v2 是全新 schema,不背 v1 增量迁移包袱。 |
| 7 | **Console 重做**:React 19 + Vite + Tailwind 4 + **shadcn/ui** + **TanStack Router** + **TanStack Query** + recharts,经 rust-embed 嵌入二进制。 | 补齐 v1 "裸 React" 缺失的路由/数据层/组件库。 |
| 8 | **砍掉 `gproxy-recorder`**。 | v2 不需要 mitm 抓包工具。 |
| 9 | **需要 v1 → v2 数据迁移**。 | 保留现网数据。 |

## 3. 模型解析与负载均衡的概念模型

这是 v2 最重要的语义设计。请求里的 model 名经过**三段清晰分层**,每段概念独立:

```
客户端 model 名
   │
   ▼  ① preprocess(预处理 / resolve)   —— 别名在这里存在且被解析掉
规范 route 名
   │
   ▼  ② route(路由)                     —— route 名 → 后端池(members)
后端池 = [member, member, ...]
   │
   ▼  ③ balance(负载均衡)               —— 已无别名,只在 members + 凭证间选择 / 故障转移
选中的 (provider, upstream_model, credential)
```

### 3.1 别名(alias)—— 预处理层

- 别名是 **name → route 名** 的多对一映射,属于"对外兼容 / 展示"关切。
- 在 **preprocess 阶段**被完全解析:别名解析 + 名称归一化 → 得到规范 route 名。
- **负载均衡阶段看不到别名**。这是关键约束:LB 核心是别名无关的,只跟 route / member 打交道,逻辑干净、可独立测试。

### 3.2 路由(route)—— 逻辑模型

- 一个 route = 对外暴露的一个逻辑模型名,背后绑 **1..N 个 member**。
  - 单 member route = 直连某 provider 的某 model。
  - 多 member route = 负载均衡池。
- route **全局定义**,只有 admin 能创建/编辑,所有用户共享。
- **权限作用在 route 名上**(用户被授权使用某个逻辑模型名;打到哪个后端是路由内部的事)。
- **池内所有 member 必须同协议**。这样故障转移时转换行为始终一致,不会换个后端就换一套转换逻辑。
- route 第一层均衡策略:`weighted` / `round_robin` / `failover`(按 tier 优先)/ `least_latency`。跳过处于熔断冷却期的 member。

### 3.3 成员(member)与凭证池 —— 后端层

- member = (provider, upstream_model_id, weight, tier, enabled)。
- 选中 member 后,在该 provider 的**凭证池**里选一把 key:
  - 跳过不健康 / 限流冷却中的 key。
  - 策略:`round_robin` / 粘性亲和(同会话粘同一 key,复用并强化 v1 的 credential affinity)。

### 3.4 协议转换归属

- **"这个上游说什么协议、要不要转换" 是 provider/channel 的属性**,不由 route 混搭决定。
- route 只在**同协议**的一组后端间做负载均衡;跨协议转换发生在 provider 层(入站协议 ≠ provider 协议时触发)。

### 3.5 两种路由模式

- **aggregated `/v1/...`**(model 名里编码):走完整 ① → ② → ③ 三段解析。
- **scoped `/{provider}/v1/...`**(URL 指定 provider):**绕过 route 直连** provider + model,仅做凭证池选择。

## 4. 顶层结构(单 crate 模块划分)

```
src/
  main.rs                 # 入口:解析参数、装配 AppState、起服务
  app.rs                  # AppState 装配 + axum Router 总装
  config/                 # bootstrap(TOML/env)解析、运行时配置快照(ArcSwap)、热更新
  api/                    # gproxy 自有 API 的端点清单 + 请求/响应形状(DTO);单一真相源
                          #   不用 OpenAPI/codegen;仅自有「管理/用户/鉴权」API
                          #   AI 协议网关端点不在此列(透传/转换,形状见 protocol/)
  http/                   # HTTP 入口(axum,仅 native)
    middleware/           # auth / classify / ratelimit / permission / sanitize
    admin/                # 管理 API handler(用 api/ 的 DTO)
    gateway/              # AI 协议代理端点 /v1、/{provider}/v1(透传/转换,无自定义形状)
    console.rs            # rust-embed 静态资源
  pipeline/               # 请求生命周期编排(替代旧 engine.execute 巨函数)
    classify.rs preprocess.rs route.rs balance.rs retry.rs execute.rs stream.rs
  protocol/               # 线格式类型 + 转换(去臃肿后)
    openai/ claude/ gemini/   # 各方言 wire 类型
    transform/            # 两两转换
      common/             # 收敛后的共享样板(SSE 分帧、role/tool 映射、usage 搬运、错误包装)
      dispatch.rs         # (from, to) → impl 转换表,替代手写巨型 match
  channel/                # 各上游渠道 + UpstreamClient 传输 trait(见 §7.4)
  store/                  # 存储抽象(两个 trait,见 §7)
    cache/                # CacheBackend trait + memory / redis 实现
    persistence/          # PersistenceBackend trait + db(SeaORM)/ file 实现
  domains/                # 域逻辑(routing/credentials/quota/ratelimit/usage/session…),组合两个 backend
  auth/ billing/          # 横切领域
```

设计原则:**任何一个文件都能单独读懂、单独测试**。文件变大即是"做了太多事"的信号。
**HTTP 端点与形状以 `api/` 下的 Rust 类型 + 路由声明为单一真相源,不引入 OpenAPI。**

## 5. 请求管线(lifecycle)

```
入站
 → auth(API key)
 → classify(协议 + 操作类型)
 → extract model
 → preprocess(别名解析 / 名称归一化 → 规范 route 名)
 → route(route 名 → 后端池)
 → permission + ratelimit + quota 预检
 → balance(选 member + 选凭证)
 → transform(若入站协议 ≠ provider 协议)
 → channel 发出
 → [失败 / 429?] retry/failover 回到 balance 选下一个
 → 计费 + 用量落账
 → 响应(passthrough 或回转协议)
```

- 每步是职责单一、可测的纯函数 / 小服务。
- `preprocess` / `route` / `balance` 是 v2 新增的三个独立步骤,正是 v1 缺失、导致做不了负载均衡的地方。
- **同协议 passthrough**:`balance` 选中的后端协议 == 入站协议时,直接 passthrough,完全不进 transform(保住 minimal-parsing 快路径)。

## 6. 协议转换层去臃肿

问题:`transform/<from>/.../<to>/...` 目录平方级爆炸。

方案(保留两两保真,收敛样板):

1. 统一 trait:`trait Transform { fn req(...); fn resp(...); fn stream(...); }`,每个有序协议对实现一次。
2. 把各转换里重复的脚手架抽到 `transform/common/`:SSE 分帧、role/tool 映射表、usage 字段搬运、错误包装。转换体只剩**真正有差异的字段映射**。
3. 用 `dispatch.rs` 的 `(from, to) → impl` 表替代 v1 中 3486 行手写巨型 match。
4. 同协议 passthrough 完全不进 transform。

预期:转换代码量明显下降;新增协议只需补"与已有协议两两"的差异映射,样板由 common 承担。

## 7. 分层存储 + 多实例

存储层**只有两个 trait 抽象**(刻意避免 v1 那种按域细分的 backend trait 过度抽象),
每个抽象有两个实现,**部署时各选一个**:

| 抽象 | 单实例(零外部依赖) | 多实例 |
|---|---|---|
| `CacheBackend` | `memory`(`DashMap`) | `redis` |
| `PersistenceBackend` | `file`(本地磁盘) | `db`(SeaORM) |

单机部署 = `memory` + `file`,**连数据库服务器都不需要**;多实例 = `redis` + `db`。
`file` 与 `memory` 一样是单实例专属(本地状态无法跨实例共享)。

### 7.1 两个 backend

**`CacheBackend`(trait)** —— 方法面:`get` / `set` / `delete` / `incr` /
`publish` / `subscribe`。`publish`/`subscribe` 用于多实例配置失效广播(memory
实现为 no-op),在多实例阶段落地。

**`PersistenceBackend`(trait)** —— typed、按域分组的方法(`upsert_provider`、
`get_route_by_name`、`find_key_by_digest`、`add_cost_used`、`append_usage`、
`add_usage_rollup`、`query_usage`、`put_file` …)。**一个 trait**,不拆成细粒度
子 trait;`db` 与 `file` 各实现一遍。

- **`db` 实现**用 SeaORM(sqlite/mysql/postgres)。**SeaORM 仅是该实现的内部细节**,
  域代码只调 trait 方法,**永不直接碰 SeaORM**。
- **`file` 实现**把数据序列化落本地磁盘(配置类小数据全量载入内存;日志/用量 append)。
- 物理上 trait 定义与两个 impl 按文件拆开,每文件 < 500 行。

### 7.2 域逻辑坐在两个 backend 之上

**没有任何域级 backend trait**(砍掉 v1 的 `RateLimitBackend` / `QuotaBackend` /
`AffinityBackend`)。ratelimit / quota / affinity / config / session / log 都是
**普通域代码**,持有 `&dyn CacheBackend` 和/或 `&dyn PersistenceBackend`。
**「分域策略」= 这个域碰哪个 backend**:

| 数据域 | 碰哪个 backend | 说明 |
|---|---|---|
| 配置 / 供应商 / 路由 / 模型 | cache + persistence(写穿) | persistence 真相源,缓存加速;改动写库→换本地 `ArcSwap` 快照→`publish` 通知其他实例失效重载 |
| 配额(钱) | cache + persistence(**弱一致**) | cache 先扣(并发下可能轻微超扣,可接受);persistence 经 usage / `user_quotas` 持久化;启动从 persistence 水合 cache,定期对账修正。**不**给 cache 加原子上限原语 |
| 限流窗口 | 只 cache | `incr` 瞬时计数,过期即弃,不持久化 |
| 凭证健康 / 熔断冷却 | cache(运行时) + persistence(审计快照) | 热状态在 cache;`credential_statuses` 留持久快照供控制台审计 |
| 用户登录态 / session | 只 cache | 不持久化 |
| 请求日志 / 用量明细 | 只 persistence | 直接落库(可异步批量写),不进 cache |
| 用量看板统计 | 只 persistence(rollup) | 按 时/天/周/月 分桶计数;看板只读 rollup,**绝不实时聚合** |

### 7.3 多实例语义

- 实例本身**无状态、可水平扩**(仅在 `redis` + `db` 组合下)。
- 共享状态全部经 `CacheBackend`(redis)+ `PersistenceBackend`(db)。
- 配置变更:写 persistence → 换本地 `ArcSwap` 快照 → `cache.publish` 广播失效 → 其他实例 `subscribe` 收到后重载快照。

### 7.4 上游传输抽象(`UpstreamClient`)

上游 HTTP 发送抽象成 **`UpstreamClient` trait**(传输可换),这是让边缘可达的关键接缝:
- **native 实现**:wreq(支持 TLS 指纹伪装 / emulation)。
- **edge 实现(后续)**:平台 `fetch`(无 TLS 控制)。

**每个 channel 声明所需传输能力**(如 `requires_tls_emulation`),且能力可细到**渠道的某种
凭证模式 / 操作**,不必整渠道一刀切。某传输不满足时对应能力自动降级:
- `chatgpt`:请求本身就需 TLS 伪装 → **仅传统常驻部署支持;serverless / 边缘标注为不支持**。
- `claudecode`:cookie→oauth 的凭证引导需伪装;但**若用户自行完成 OAuth、直接提供 oauth
  token**,则无需伪装 → 边缘可用(**仅 token 模式**;cookie 自动换取功能在边缘不可用)。
- codex / 各 API-key 类:无伪装需求,边缘可用。

具体每渠道(及其各凭证模式)的能力在实现时由各 channel 自行声明,架构按能力自动降级,
不靠预先把清单列全。

## 8. 数据模型(逻辑记录)

v2 是**逻辑数据模型**:`db` 实现用 SeaORM 表实现它(全新 schema,**不考虑 v1 迁移兼容**),
`file` 实现用本地文件实现同一份逻辑数据。下列即逻辑记录(`PK=id i64`、
`created_at/updated_at` 默认有,不再重复)。

**A. 路由 / 模型**
- `routes`:`name`(唯一)· `strategy`(weighted/round_robin/failover/least_latency)· `enabled` · `description?`
- `route_members`:`route_id` · `provider_id` · `upstream_model_id` · `weight` · `tier` · `enabled`
- `aliases`:`alias`(唯一)· `route_id`(多对一)
- `provider_models`:`provider_id` · `model_id` · `display_name?` · `pricing_json?` · `enabled`

**B. 供应商 / 凭证**
- `providers`:`name`(唯一)· `channel` · `label?` · `settings_json`(base_url 及各 channel 标量开关)· `credential_strategy` · `enabled` —— **不再有任何 rules 的 JSON 列**,全部提成下列独立表
- `credentials`:`provider_id` · `name?` · `kind` · `secret_json`(加密)· `weight`(凭证池)· `enabled`
- `credential_statuses`:`credential_id` · `channel` · `health_kind` · `health_json?` · `checked_at?` · `last_error?` *(审计快照)*

**B2. 供应商级规则(全部独立表,结构化、可逐行编辑/审计;均含 `provider_id` · `sort_order` · `enabled`)**
- `routing_rules`:`operation` · `protocol`(入站)· `implementation`(passthrough/transform_to/local/unsupported)· `dest_operation?` · `dest_protocol?` — 唯一约束 `(provider_id, operation, protocol)`
- `rewrite_rules`(JSON 字段操作):`path`(点路径)· `action`(set/remove)· `value_json?`(set 时)· `filter_model_pattern?` · `filter_operations?` · `filter_protocols?`
- `sanitize_rules`(正文正则替换):`pattern`(正则)· `replacement`
- `cache_breakpoints`(Claude 缓存):`target` · `position` · `index` · `ttl` *(magic-string 触发器是内置常量,非配置)*
- `beta_headers`:`token`(`anthropic-beta` 能力标志,如 `oauth-2025-04-20`)
- `preludes`:`text`(注入首个 system 块的前导文本;v1 单条,v2 支持按 `sort_order` 多条)

**C. 用户 / 鉴权 / 权限 / 限额**
- `users`:`name`(唯一)· `password?`(hash)· `enabled` · `is_admin`
- `user_keys`:`user_id` · `api_key_ciphertext` · `api_key_digest`(唯一索引)· `label?` · `enabled`
- `user_route_permissions`:`user_id` · `route_pattern`(glob,作用在 route 名上)
- `user_rate_limits`:`user_id` · `route_pattern` · `rpm?` · `rpd?` · `total_tokens?`
- `user_quotas`:`user_id`(唯一)· `quota_total` · `cost_used`(对账后持久值)
- `user_file_permissions`:`user_id` · `provider_id`

**D. 用量 / 日志(只持久化)**
- `usages`(明细,append):`at` · `route_name?` · `provider_id?` · `credential_id?` · `user_id?` · `user_key_id?` · `operation` · `protocol` · `model?` · `input/output_tokens` · `cache_read/creation_tokens`(+5min/1h)· `cost`
- `usage_rollups`(看板源):`granularity`(hour/day/week/month)· `bucket_start` · 维度(`provider_id?` / `user_id?` / `route_name?` / `model?`)· 指标(`requests` / `input_tokens` / `output_tokens` / `cost`)。每请求 `add_usage_rollup` 累加
- `downstream_requests` / `upstream_requests`:抓包日志(受 enable 开关),沿用 v1 结构(下行 path/query,上行 url/latency)

**E. 设置**
- Bootstrap(TOML/env,连库前就要):`dsn?` · `redis_url?` · `instance_name?` · `host?` · `port?`(`dsn` 缺省即 `file` 持久化模式)
- `instance_settings`(按 `instance_name` 每实例一行,单实例一行/多实例多行):`proxy?` · `spoof_emulation?` · `enable_usage` · `enable_upstream_log(_body)` · `enable_downstream_log(_body)` · `update_channel?`

**F. 文件**
- `files`:`provider_id` · `file_id` · `filename` · `mime_type` · `size_bytes` · `downloadable?` · `raw_json`(元数据)
- blob 内容随当前 `PersistenceBackend` 实现存储(`file` 落磁盘 / `db` 落库)

## 9. Console 技术栈

- React 19 + Vite + Tailwind 4
- **shadcn/ui**(拥有源码的组件,无运行时黑盒)
- **TanStack Router**(类型安全路由)
- **TanStack Query**(API 拉取 / 缓存 / 失效,天然配合多实例配置变更)
- recharts(图表)
- 构建产物经 rust-embed 嵌入二进制,保持单文件部署。

## 10. 分阶段实施

大工程,按里程碑拆,每阶段独立可跑、可测:

1. **骨架**:单 crate 脚手架 + AppState + 配置 + 存储抽象(先 memory)。
2. **管线 + passthrough**:auth / classify / preprocess / route / balance / execute,先只做同协议 passthrough。**此阶段引入 `UpstreamClient` trait(native wreq 实现)+ 渠道传输能力标记**(§7.4 边缘接缝)。
3. **协议转换移植**:trait + common 收敛,逐对迁移转换逻辑。
4. **负载均衡**:两层池 + 熔断 / 凭证冷却。
5. **多实例**:Redis 后端 + 配置 pub/sub 失效。
6. **Console 重做**。
7. **数据迁移**:v1 DB → v2 schema 的一次性迁移脚本。
8. **边缘 / WASM 构建(后续,见 §13)**:`UpstreamClient` 的 fetch 实现 + 两个 backend 的 HTTP 实现(Upstash/Turso)+ wasm 构建 + 各平台 fetch 入口。`chatgpt` 渠道在此构建不可用。

## 11. 代码规范(实施期强制)

- **文件大小**:单文件理想 **≤200 行**,**绝对不超过 500 行**。确需超过的,先经人工审批。
- **结构化组织**:按职责拆分模块,文件变大即视为"做了太多事"的信号,及时拆分。
- **写前检查**:动手前先搜索代码库是否已有部分实现可复用/扩展,并评估是否需要先做小范围重构,避免重复实现。
- **格式与 lint**:每次写完/改完代码,收尾必跑 `cargo fmt` 与 `cargo clippy`,clippy 告警要修而非压制(除非有明确理由)。
- **测试克制**:**不做默认 TDD,不写大量冗余测试,避免过度测试**。仅对真正棘手的逻辑、真实 bug 的回归点写精简测试。

## 12. 开放问题

- v1 → v2 数据迁移的具体字段映射,待 v2 schema 定稿后单独成文。
- 各 channel 的移植优先级排序。**`chatgpt` 渠道先不实现**(唯一刚需 TLS 伪装者,推后);
  其余渠道优先。`UpstreamClient` 接缝与能力标记仍照建,后续补 chatgpt 是干净加法。

## 13. 边缘 / WASM 支持(后续目标)

目标平台:Cloudflare Workers、阿里云 ESA、腾讯 EdgeOne Pages、Vercel Edge、
Netlify Edge —— 这五家都是 V8 isolate / WASM 运行时,统一走 **WinterCG Web Fetch**
入口,核心编译一份 `wasm32`,每平台一层薄 glue。(Vercel/Netlify 另有原生
Serverless Functions,那条等于 Lambda 适配器,全功能、非边缘。)

**功能差异(按渠道能力自动降级,见 [§7.4](#74-上游传输抽象-upstreamclient))**:
- `chatgpt`:**serverless / 边缘均不支持**(请求刚需 TLS 伪装),仅传统常驻部署可用。
- `claudecode`:边缘可用,但**仅 token 模式**(需用户自行完成 OAuth 提供 token;
  cookie→oauth 自动换取需伪装,边缘不可用)。
- codex / 各 API-key 类:边缘全可用。

**为边缘需要做的(大多是加实现,非重写)**:
1. `UpstreamClient` 的 `fetch` 实现(§7.4 接缝已留)。
2. `CacheBackend` / `PersistenceBackend` 的 HTTP 实现:Upstash Redis(HTTP)+
   Turso/libSQL(HTTP),一份 wasm 五平台通吃,不绑各家私有 binding。
3. wasm 构建 + 各平台 fetch 入口适配器(薄)。
4. `Send` 边界:wasm 上用 `#[cfg_attr(target_arch="wasm32", async_trait(?Send))]`,
   域代码不受影响(成熟套路,低成本)。
5. 后台任务(对账/清理)做成可被宿主调度的函数(native tokio / edge cron),
   核心不写死常驻循环。

**仍然不做的**:不搞通用"可换 HTTP 宿主"抽象层——native 用 axum、edge 用 fetch 入口,
是**两个具体适配器**(cfg 分目标),不是一层抽象税。核心
(`pipeline / protocol / route / balance / backends`)不依赖 axum。

**节奏**:native 先交付;边缘作为后续阶段。v2 核心现在就埋好接缝(§7.4 的
`UpstreamClient` trait + 渠道能力标记、后台任务不写死),使边缘是"加实现"而非"重写"。
