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
- **权限 / 配额 / 限流按 `user → team → org` 多级解析**:权限取三级**并集**;配额、限流三级**逐级预检**,最严格者拦截(详见 §8-C)。
- **池内所有 member 必须同协议**。这样故障转移时转换行为始终一致,不会换个后端就换一套转换逻辑。
- route 第一层均衡策略:`weighted` / `round_robin` / `failover`(按 tier 优先)/ `least_latency`。跳过处于熔断冷却期的 member。
- **熔断阈值可配**:连续失败数 **或** 滑动窗口错误率触发熔断 + 冷却时长(过冷却自动半开探测)。默认在 **provider 级**配置,route 级可覆盖(见 §8-B `providers.settings_json`)。`least_latency` = 上游 send 延迟 EWMA(α=0.3),未试探过的 member 优先(乐观);route 级熔断覆盖待 routes 增加 settings 列后补。

### 3.3 成员(member)与凭证池 —— 后端层

- member = (provider, upstream_model_id, weight, tier, enabled)。
- 选中 member 后,在该 provider 的**凭证池**里选一把 key:
  - 跳过不健康 / 限流冷却中的 key。
  - 策略:`round_robin` / **粘性亲和**(`sticky`)。
- **sticky 亲和 key**:默认按**入站 user key**(同一调用方粘同一凭证,复用并强化 v1 的 credential affinity);无 user key 时回退按 `session`(若有)。亲和映射存 cache(redis),TTL 滚动。
- **凭证冷却语义是渠道强相关的,待研究细化(2026-06-10 记)**:不同渠道的"该歇多久"差异很大——
  429 的含义各家不同(瞬时限速 vs 分钟/日配额耗尽 vs 账号级封禁)、`retry-after` 可信度不一、
  订阅型账号(OAuth/cookie 渠道)有固定配额窗口与"封禁期"模式、部分渠道按账号而非按 key 限制。
  M4 现状是统一简化策略(429 按 retry-after 否则 30s;AuthDead 600s)。后续(随 M7 各 OAuth
  渠道落地)逐渠道研究,预期出口:渠道级冷却策略钩子(`Channel` trait 方法或渠道配置,
  如自定义 429 解析、配额窗口对齐、账号级冷却联动同 provider 其余 key)。
- **各渠道已知冷却信号(2026-06-11 记,随 M7b 调研)**:目前只记录信号,逐渠道冷却钩子仍是
  后续工作(各渠道冷却调优时再落地解析)——
  - **codex**:响应头 `x-codex-primary-reset-at` / `x-codex-secondary-reset-at`(主/次配额窗口
    重置时刻)给出比 `retry-after` 更精确的冷却到期;`x-codex-credits-has-credits: false` 表示
    额度耗尽,应按 `AuthDead`(标死换 key,而非短冷却)处理。
  - **claudecode**:响应头 `anthropic-ratelimit-unified-reset`(统一限流重置时刻)是冷却到期的
    权威来源。
  - **kiro**:429 触发冷却;另有 endpoint 级回退(同 provider 多上游主机)的容错维度。
  - **google(geminicli / antigravity / vertex)**:429 + `retry-after`,按标准 Google 配额语义对齐。
  - **copilotcli / github**:GitHub 速率头(`x-ratelimit-remaining` / `x-ratelimit-reset`)给出
    窗口剩余与重置时刻。

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
  config/                 # bootstrap(env + CLI,无配置文件;Arc 不可变)。运行时设置快照(ArcSwap)属控制面,后续阶段建
  api/                    # gproxy 自有 API 的端点清单 + 请求/响应形状(DTO);单一真相源
                          #   不用 OpenAPI/codegen;仅自有「管理/用户/鉴权」API
                          #   AI 协议网关端点不在此列(透传/转换,形状见 protocol/)
                          #   【已落地 M10a】api/auth(login/me DTO)+ api/error(ApiError);axum-free,含 wasm edge
  http/                   # HTTP 两端(按方向拆)
    client/               # 出站传输:UpstreamClient trait + wreq(native)/ fetch(wasm)(见 §7.4)
    server/               # 入站(native,axum):router + health + middleware/ + admin/ + gateway/ + console
                          #   middleware: auth/classify/ratelimit/permission(均为入站、provider 无关)
                          #   admin: 管理 API handler(用 api/ 的 DTO);gateway: /v1、/{provider}/v1 透传/转换
                          #   【已落地 M10a】admin/{mod,middleware,auth}:session 中间件 + login/logout/me
    edge.rs               # 入站 wasm:WinterCG fetch 入口,驱动同一 Router(见 §13)
  pipeline/               # 请求生命周期编排(替代旧 engine.execute 巨函数)
    classify.rs preprocess.rs route.rs balance.rs   # 入站 → 选后端各步
    execute.rs            # 执行层:泛型编排 transform→process→channel→send→classify→normalize→transform-resp→usage(见 §6.3)
    failover.rs           # 上游 failover + 凭证轮换,绕着 send(见 §6.4)
    stream.rs             # 流式响应尾段(逐帧 normalize/transform/tee/usage)
  protocol/               # 线格式类型 + 转换(去臃肿后)
    openai/ claude/ gemini/   # 各方言 wire 类型
    transform/            # 两两转换
      common/             # 收敛后的共享样板(SSE 分帧、role/tool 映射、usage 搬运、错误包装)
      dispatch.rs         # (from, to) → impl 转换表,替代手写巨型 match
  process/                # provider 规则处理层(transform 之后、channel 之前,见 §6.1)
                          #   system_text / cache_breakpoint / rewrite / sanitize / header
                          #   作用于 provider-native 请求(headers+body);channel 因此保持纯接入
  channel/                # (后续)按【供应商】的纯接入适配:Channel trait + openai/claude/codex/...
                          #   只管 auth 注入 + endpoint/method + 传输能力声明;用 http::client 发请求
                          #   不放传输、也不放规则改写(规则在 process/)
  store/                  # 存储抽象(两个 trait,见 §7)
    cache/                # CacheBackend trait + memory / redis 实现
    persistence/          # PersistenceBackend trait + db(SeaORM)/ file 实现
  domains/                # 域逻辑(routing/credentials/quota/ratelimit/usage/session…),组合两个 backend
  auth/ billing/          # 横切领域
  migrate/                # 配置导出/导入(CLI/首启,见 §18)
```

设计原则:**任何一个文件都能单独读懂、单独测试**。文件变大即是"做了太多事"的信号。
**HTTP 端点与形状以 `api/` 下的 Rust 类型 + 路由声明为单一真相源,不引入 OpenAPI。**

## 5. 请求管线(lifecycle)

```
入站
 → 生成 request_id(ULID,贯穿全程,见 §15)
 → auth(API key)
 → classify(协议 + 操作类型)
 → extract model
 → preprocess(别名解析 / 名称归一化 → 规范 route 名)
 → route(route 名 → 后端池)
 → permission + ratelimit + quota 预检(配额先扣估算,见 §17)
 → balance(选 member + 选凭证)
 → transform(若入站协议 ≠ provider 协议)
 → process(provider 规则改写:system_text/cache_breakpoint/rewrite/sanitize/header,见 §6.1)
 → channel(纯接入:auth + endpoint,见 §6.3)
 → UpstreamClient 发出(按凭证选 proxy,见 §7.4)
 → [失败 / 429?] failover:回到 balance 选下个凭证重试(见 §6.4)
 → 计费 + 用量落账(以 request_id 幂等,仅成功计费;对账,见 §17)
 → 响应(passthrough 或回转协议)
```

- 每步是职责单一、可测的纯函数 / 小服务。
- `preprocess` / `route` / `balance` 是 v2 新增的三个独立步骤,正是 v1 缺失、导致做不了负载均衡的地方。
- **同协议 passthrough**:`balance` 选中的后端协议 == 入站协议时,直接 passthrough,完全不进 transform(保住 minimal-parsing 快路径)。
- **横切关切**:request_id / 可观测性见 §15,安全(密钥/鉴权/脱敏/TLS)见 §14,优雅停机/过载/入站防护见 §16,计费幂等见 §17。

## 6. 协议转换层去臃肿

问题:`transform/<from>/.../<to>/...` 目录平方级爆炸。

方案(保留两两保真,收敛样板):

1. 统一 trait:`trait Transform { fn req(...); fn resp(...); fn stream(...); }`,每个有序协议对实现一次。
2. 把各转换里重复的脚手架抽到 `transform/common/`:SSE 分帧、role/tool 映射表、usage 字段搬运、错误包装。转换体只剩**真正有差异的字段映射**。
3. 用 `dispatch.rs` 的 `(from, to) → impl` 表替代 v1 中 3486 行手写巨型 match。
4. 同协议 passthrough 完全不进 transform。

预期:转换代码量明显下降;新增协议只需补"与已有协议两两"的差异映射,样板由 common 承担。

### 6.1 处理层(process)—— 渠道与规则分离

**问题(v1 粗糙处)**:provider 级的请求改写规则(prelude/cache/rewrite/sanitize/beta_headers)
在 v1 里和"渠道接入"揉在一起;且 §4 一度把 `sanitize` 错列为**入站 middleware**——但这些规则是
**按 provider 配置**的(§8-B2),而 provider 要到 `balance` 之后才选定,入站阶段根本跑不了。

**方案**:独立 `process/` 层,夹在 **transform 之后、channel 之前**,作用于已转好的
**provider-native 请求(headers + body)**:

| §8-B2 规则 | 归属 |
|------|------|
| `routing_rules`(passthrough/transform_to/local/unsupported) | **transform-dispatch**(决定要不要转、转成啥),不在 process |
| `rule_sets` → `rules`(system_text / cache_breakpoint / rewrite / sanitize / header),经 `provider_rule_sets` 挂到 provider | **process 层**,按固定顺序作用(system_text → cache_breakpoint → rewrite(JSON path)→ sanitize(正文正则)→ header(头)) |

**规则解析**:选定 provider → 取其 `enabled` 的 `provider_rule_sets`(按 `sort_order`)→ 各 set 内 `enabled` 的 `rules`(按 `sort_order`)→ 用 `filter_model_pattern`(匹配**去前缀后的 model**)与 `filter_operation_keys`(匹配当前 operation)过滤 → 按 `kind` 作用。

效果:**channel 层退化为纯接入**——只做 auth 注入 + endpoint/method + 传输能力声明,交给
`UpstreamClient` 发;规则改写完全不进 channel。

### 6.2 reasoning 签名兼容(跨 provider)

各家的"思考/推理"块带**不透明签名**:Claude thinking-block signature、OpenAI encrypted_content、
Gemini thought signature。多轮对话里这些签名会随历史回传;当一个带 A 家签名的请求被路由到
**另一账号 / 另一 provider** 时,签名可能不被接受。需要一层**签名兼容判定**:`DetectProvider`
→ `preserve / drop / replace`(目标兼容则保留;不兼容则丢弃或替换为目标可接受的占位/旁路 sentinel)。
配套一个**思考签名缓存**(按内容 hash + 模型族,短 TTL)避免重复签名。归属在 transform/process 边界,
仅在**跨 provider 路由**时触发,同协议 passthrough 不涉及。

### 6.3 执行层(executor)与 channel trait

**定位**:executor = 一条**泛型编排管线**(`pipeline/execute.rs`),把 §5 里
`transform → process → channel → send → classify → normalize → transform-resp → usage`
这串**已各自成层**的步骤按序串起,failover(§6.4)套在 send 周围。对所有渠道**只有一份实现**,
渠道差异全在它持有的 `Arc<dyn Channel>` 上。

**反面教训(两边都不照抄)**:

- 不学 CPA 的 **per-provider 胖 executor**(把翻译 + 改写 + 发 + 反翻译全塞进每个 provider 的
  `Execute/ExecuteStream`)—— 与 v2 已拆出的 transform/process 分层冲突,加协议要改 N 处。
- 不学 v1 的 `execute_inner` / `execute_stream_inner` 两个 ~600 行**孪生上帝函数**。

**Channel trait —— 薄,纯接入**(§6.1 定调;比 v1 的 Channel 瘦一圈):

| 方法 | 职责 |
|---|---|
| `prepare(cred, settings, native_req) -> PreparedRequest` | 注入 auth、定 endpoint/method;v1 的 `finalize_request`(默认 model、request-id 注入等渠道语义归一)**并入此处** |
| `classify(status, headers, body) -> Disposition` | 上游响应分类(五态,见 §6.4) |
| `normalize(resp) -> Bytes` | transform 前对上游原始正文的渠道特定修正 |
| `needs_refresh / refresh(client, cred)` | 凭证刷新(见 §14.5) |
| `requires_tls_emulation()` / 传输种类(HTTP \| WS) | 传输能力声明,按能力降级(见 §7.4) |

**不含** transform、rules、pricing、token-count —— 分别归 transform 层 / process 层(§6.1)/ 其他域。

**派发**:`HashMap<channel_id, Arc<dyn Channel>>` 启动期建好(registry,无 v1 式大 match);
解析出的 `provider.channel` → `Arc<dyn Channel>`。

**流式 / 非流式合一**(修 v1 头号痛点):send 前与 classify **全共享**(流式仅 peek
status/headers,出错才 buffer 正文),**只在出响应正文一步分叉**。统一出参:

```
ExecOutcome { status, headers, body: Full(Bytes) | Stream(ByteStream), disposition }
```

- 非流式:`buffer → normalize → transform.resp → usage`
- 流式(`pipeline/stream.rs`):`包流 → 逐帧(normalize → transform.stream → tee 原始正文供日志 → usage 观察)`

前半段同一码路,干掉 v1 两个孪生函数。

**short-circuit**:routing 的 `local`(不发上游)在 transform-dispatch 阶段就短路,
**不进 executor 的上游调用** —— 目前两类 local 实现:
- **models**:聚合 `/v1/models` 返回**网关视角的可用模型 = alias/route 名列表**(永不发上游);
  scoped `/{provider}/v1/models` 按 routing_rule——`local` = 仅 `provider_models`+变体展开,
  `passthrough/transform` = 上游列表转换后**合并**手动行与变体。GetModel 同理。
  `GET /v1/models` 是 openai/claude 撞路径,classify 按**入站凭证形态**消歧
  (`x-api-key` → claude,其余 → openai)。
- **count_tokens 本地计数**:`src/tokenize/` + **全局词库注册服务**(TokenizerRegistry,挂 AppState):
  内置 tiktoken 编码(gpt 族启发式:o200k/cl100k)+ **打包 deepseek-v4-pro tokenizer**
  (非 gpt 默认计数器);词库经**持久层统一存取**(file 后端 = `data_dir/tokenizers/`
  原始文件;db 后端 = BLOB 表 `tokenizer_vocabs`),registry 内存缓存 + 后台
  hydrate/下载;注册服务可枚举词库
  (内置/打包/已下载,M10 管理 API 暴露)。模型→词库映射:
  `provider.settings_json.tokenizer_map`(glob → 词库名或 HF repo)。
  在线下载:`instance_settings.enable_tokenizer_download`(默认关);开启时映射指向的
  缺失词库**后台**从 HF 拉取(走 `UpstreamClient`,经出站代理),本次请求先按退化链兜底。
  **退化链(非 gpt)**:映射词库 → 内置 deepseek-v4-pro → (无 tokenizers 服务,如 edge)
  字符估算 chars/2。
  **默认规则**:CountTokens 在 claude/gemini 渠道走上游真端点(openai 官方有
  `/v1/responses/input_tokens`,可显式配 passthrough),openai 兼容族默认 local;
  且 failover 全部候选失败时 local **兜底回退**。响应按入站协议形态构造。

**WS-out**:按 §7.4 —— `UpstreamClient` 出双工帧管道、**帧协议归 channel**、解回内部统一 stream
事件,**executor 不为 WS 特判**(只问 channel 传输种类);会话复用另加一个**可选** `SessionCloser`
子 trait。实现随 codex / TLS 伪装一并推后(§12),本期只留接缝。

### 6.4 上游 failover 与凭证轮换

**定位**:独立模块 `pipeline/failover.rs`(executor 调用),**绕着 send**,既不进 channel 也不进
executor 主体 —— v1(`retry.rs`)与 CPA(conductor Manager)一致这么分。仅靠 channel 的
`prepare + classify` 两个钩子即对所有渠道通用;**流式非流式共用同一 loop**(只 send 那一下不同)。

**`Disposition`(五态)**:`classify` 产出,驱动 failover + 冷却 + 凭证健康 + 计费(§17):

| 态 | 触发 | 动作 |
|---|---|---|
| `Success` | 2xx | 返回;标该凭证健康 |
| `AuthDead` | 401/402/403 | `refresh` 刷新一次;仍失败 → 标死、换下个凭证 |
| `RateLimited { retry_after }` | 429 | 有 `retry_after` → 冷却该凭证、换下个;无 → 同凭证有限重试 |
| `Transient` | 5xx / 网络错 | 换下个凭证 |
| `Permanent` | 4xx 校验类 | 立即返回,不重试 |

- **候选顺序**来自 balance(§3.3 凭证池策略 round_robin / sticky)。
- **凭证健康 / 熔断冷却**按 §7.2 走**本地内存软启发式**;可选定期刷 `credential_statuses` 供审计。
- 每次失败尝试产出一条 `upstream_requests` 失败元信息(真实 URL / 头 / 正文 / 响应),供日志(§8-D / §15)。
- **与计费衔接**:failover 的失败尝试**不计费**,仅最终 `Success` 那次按 request_id 幂等计费(§17)。

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
`add_usage_rollup`、`query_usage` …)。**一个 trait**,不拆成细粒度
子 trait;`db` 与 `file` 各实现一遍。

- **`db` 实现**用 SeaORM(sqlite/mysql/postgres)。**SeaORM 仅是该实现的内部细节**,
  域代码只调 trait 方法,**永不直接碰 SeaORM**。
- **`file` 实现**把数据序列化落本地磁盘(配置类小数据全量载入内存;日志/用量 append)。
- 物理上 trait 定义与两个 impl 按文件拆开,每文件 < 500 行。

### 7.2 域逻辑坐在两个 backend 之上

**没有任何域级 backend trait**(砍掉 v1 的 `RateLimitBackend` / `QuotaBackend` /
`AffinityBackend`)。ratelimit / quota / affinity / config / session / log 都是
**普通域代码**,持有 `&dyn CacheBackend` 和/或 `&dyn PersistenceBackend`,外加进程内的
**控制面快照(`ArcSwap`)**。**「分域策略」= 这个域把状态放哪**。

`CacheBackend` 是**扁平的共享缓存**(memory 单 / redis 多),**不做 memory→redis→db
分层,db 永不是 cache 的回落层**。多实例下实例的"本地内存"只有两块,且都可从
persistence 重建(丢了不影响正确性,实例仍逻辑无状态):**① 控制面 ArcSwap 快照**、
**② 凭证健康 / member 熔断这类软启发式**。

| 数据域 | 放哪(多实例) | 说明 |
|---|---|---|
| 控制面(配置 / providers / routes / aliases / 规则表 / orgs / teams / users / keys / 权限) | **本地 ArcSwap 快照** + persistence 真相源 + pub/sub 失效 | 读多写少,每请求多次查;失效广播保一致 |
| 凭证健康 / 熔断冷却 + **LB member 熔断** | **本地内存**(软启发式);可选定期刷 `credential_statuses` 供审计 | 各实例自愈,无需全局一致;免去热路径 redis 往返 |
| 限流计数 + **凭证级 RPM/TPM 预算** | **redis 直连**(memory 单实例) | 必须全局求和,本地会漏判;凭证预算同理(每 key 一组计数器) |
| 配额(钱) | **redis 直连**(弱一致),**按 user/team/org 三级各一组计数器** | cache 先扣,本地会 N 倍超扣;persistence 经 usage/`quotas` 持久化,启动水合 + 定期对账 |
| 用户登录态 / session | **redis 直连**(或粘性路由) | 请求可落任意实例 |
| 请求日志 / 用量明细 | 只 persistence | 直接落库(可异步批量写) |
| 用量看板统计 | 只 persistence(rollup) | 按 时/天/周/月 分桶;看板只读 rollup,**绝不实时聚合** |

### 7.3 多实例语义

- 实例**逻辑无状态、可水平扩**(`redis` + `db` 组合下)。本地内存仅控制面快照 +
  健康/熔断启发式,二者皆可重建。
- 必须全局一致的状态(限流 / 配额 / session)**只经 redis**,实例不留本地副本(杜绝脏读)。
- 配置变更:写 persistence → 换本地 `ArcSwap` 快照 → `cache.publish` 广播失效 →
  其他实例 `subscribe` 收到后从 persistence 重载快照。

**M8 多实例落地(2026-06-11)**:失效走单一 redis 频道 `gproxy:invalidate`(常量
`INVALIDATE_CHANNEL`)。`app/invalidation.rs::spawn_invalidation_listener`(**native-only**,
仅 redis 缓存时由 `main.rs` 在 router 消费 state 之前 spawn)`subscribe` 该频道,每条消息触发一次
全量 `reload_snapshot()`。一致性原语仍是 **`version` + `ArcSwap`**:`reload_snapshot` 从
persistence 重建快照、`version.wrapping_add(1)`、`ArcSwap::store` 原子换入——多条失效消息「后写胜出」
天然安全,无快照结构变更。memory 单实例 `subscribe` 为 no-op。

**edge 惰性失效(2026-06-12)**:edge 无长连接、`subscribe` 为 no-op,曾导致温热 isolate 的快照
永不刷新(增删 key / 改配置在 edge 上不可见,直到 isolate 回收)。现改**版本戳轮询**:两个广播点
(admin 变更 `admin::invalidate`、凭证刷新回写 `credentials/refresh`)统一走
`app/invalidation.rs::broadcast` —— publish 之外原子 `incr` 共享键 `gproxy:cfg-version`
(常量 `CONFIG_VERSION_KEY`);edge 在 fetch 路径上按 **≥10s 节流**(`SNAPSHOT_POLL_INTERVAL_MS`)
`incr 0` 读取,版本变动即 `reload_snapshot()` 换入(失败保留旧版本号、下个窗口重试);`init` 时记
基线防首请求空转。代价:每 10s 至多一次 Upstash GET、变更后最坏 ~10s 陈旧窗口。
subscribe 断连重连、失效合并是后续项。

### 7.4 上游传输抽象(`UpstreamClient`)

上游 HTTP 发送抽象成 **`UpstreamClient` trait**,**与具体 HTTP 客户端无关**(trait 不绑 wreq)——这是让边缘可达、客户端可换的关键接缝。两个实现,按构建目标 cfg 选:
- **native 实现 = `WreqClient`(wreq)**:统一走 wreq(它本身就能发普通请求,也能做 TLS 指纹伪装),不为非伪装渠道另搞客户端。
- **edge 实现 = `FetchClient`(平台 `fetch`)**:wasm 目标下用,无 TLS 控制(故 `chatgpt` 在 edge 不可用)。

渠道按能力(`requires_tls_emulation` 等)声明需求;native 全部由 wreq 满足,edge 由 fetch 满足(不满足伪装的渠道自动降级)。
`AppState` 持有 `Arc<dyn UpstreamClient>`;native 启动时装配 `WreqClient`,
edge 初始化时装配 `FetchClient`,业务执行层只调 trait。

**每凭证不同出站代理 / TLS 指纹**:wreq 的 proxy 与 impersonation 绑在 client 实例上、无法逐请求切换,所以
native 的 `WreqClient` 内部维护一个**按 `(proxy_url, tls_fingerprint)` 缓存的 client 池**,
发请求时按**选中凭证**解析出的 (proxy, 指纹) 选用对应 client(懒构建 + 复用)。解析(见 `channel::resolve`):
**proxy = credential 覆盖 ?? provider 默认 ?? 全局默认**(`--upstream-proxy-url` / `GPROXY_UPSTREAM_PROXY_URL`);
**TLS 指纹 = credential 覆盖 ?? provider 默认**(均为 provider 实例级字段,见 §8-B)。
**edge 不支持本地 proxy / 伪装**,走平台 `fetch`,两者在 edge 忽略。

**M7a**:client 池已按 (proxy, fingerprint) 键;header 层 emulation 生效。wreq 6.0.0-rc.29。

**指纹三层全部落地(2026-06-12)**:`fingerprint::to_emulation` 现在映射 `tls_fingerprint` 的三层 ——
`headers`(默认头/UA)、`tls`(→`wreq::tls::TlsOptions`:ALPN/GREASE/版本/cipher/curves/sigalgs/扩展序)、
`http2`(→`wreq::http2::Http2Options`:SETTINGS 值+`settings_order`、连接窗口、`headers_pseudo_order` 伪头序 =
Akamai h2 指纹;`"http2": false` 表 HTTP/1.1-only,由 ALPN 落实)。抓包/草案见 `docs/agent-tls-fingerprints.md`
§5/§6(claude/kiro/gemini/copilot 模型路径 = http/1.1;只有 codex/agy 走 h2)。**BoringSSL 保真上限**:UA+ALPN+
关GREASE+套件近似可精确,OpenSSL/Go/rustls 栈的字节级 JA3/JA4 复刻不了;字节级 Akamai 保真需对编译产物实抓验证。

**M7b 各渠道伪装画像(2026-06-11)**:**impersonation 类(claudecode / codex)需贴合官方 CLI 的 TLS/HTTP2
指纹**——它们随选中凭证的 `tls_fingerprint` 走上面的 client 池(native;edge 仅 token/header 模式可用)。
**其余 5 个(vertex / copilotcli / geminicli / antigravity / kiro)是 header-only**:只注入鉴权 + UA/客户端指纹头,
不要求 TLS 伪装,故 native/edge 均可(各渠道仍可经 provider/credential 配 `tls_fingerprint` 选择性启用)。

**M7b 待办 —— UA 归并进指纹配置(2026-06-11 记)**:目前各 OAuth 渠道的 UA 是**硬编码 `const` + `HeaderValue::from_static`**
注入(`codex/auth.rs`、`claudecode/auth.rs`、`geminicli`、`antigravity`、`kiro`、`copilotcli`),与 TLS/HTTP2 指纹分离。
M7b 要把 UA 收进 `tls_fingerprint` JSON 的 `headers` 层(已生效的那层),让"UA + TLS + HTTP2 + header 序"成**一套可运行时改的指纹**,
不再各渠道散落硬编码。具体三项:
  1. **UA 纳入指纹配置**:渠道硬编码 UA 降级为"无指纹配置时的兜底默认",`tls_fingerprint.headers.user-agent` 一旦存在即覆盖。
  2. **去掉硬编码 UA 里的 `gproxy` 字样**:`codex_cli_rs/0.118.0 (... x86_64) gproxy`、`KiroIDE-0.12.224-gproxy` 这类掺了
     `gproxy` 的 UA 对"伪装成真客户端"是反效果(真客户端不带 gproxy),改回纯净官方 UA。
  3. **API-key 渠道(openai / claudeapi / deepseek / …)给一个合理默认 UA**:现在它们经 `allow_headers` 默认拒绝 + 入站 UA 被全局黑名单剥掉
     → 完全不带 UA,裸用 wreq 默认 UA。应给一个中性默认(或随指纹配置),避免被识别为非常规客户端。
  4. **claudecode 的 cookie→code 引导流程记为 chrome 指纹**:cookie 换取走的是浏览器路径(`claude.ai` cookie),应伪装成
     **chrome**(TLS + UA 一整套),而非 `claude-cli/...`;chat 主路径仍用 `claude-cli` 客户端指纹。即同一渠道按"引导 vs 推理"用不同指纹。

**是否 TLS 伪装由 provider/credential 是否配置 `tls_fingerprint` 决定**;某传输不满足时对应能力自动降级:
- `chatgpt`:请求本身就需 TLS 伪装 → **仅传统常驻部署支持;serverless / 边缘标注为不支持**。
- `claudecode`:cookie→oauth 的凭证引导需伪装;但**若用户自行完成 OAuth、直接提供 oauth
  token**,则无需伪装 → 边缘可用(**仅 token 模式**;cookie 自动换取功能在边缘不可用)。
- codex / 各 API-key 类:无伪装需求,边缘可用。

具体每渠道(及其各凭证模式)的能力在实现时由各 channel 自行声明,架构按能力自动降级,
不靠预先把清单列全。

**WebSocket 上游(WS-out,代理作 WS 客户端连上游)**:少数渠道出站走 WebSocket 而非 HTTP/SSE(如 Codex websockets)。
`UpstreamClient` 除 `send`(HTTP)外提供一个 **WS 双工帧管道**(connect/收发帧);**WS 消息协议归该 channel**
(把 provider-native 请求包成帧、把流式帧解析回内部统一 stream 事件,transform/response 那套照常复用)。存活用
ping/pong + read deadline(连接建立后唯一允许的超时)。**硬骨头是"WS over 伪装 TLS"**——若渠道需指纹伪装,
握手也要走伪装栈,故 **WS-out 实现与 codex / TLS 伪装一并推后(§12),本期只留接缝**。edge 视平台 `WebSocket`
能力,不支持则该渠道在 edge 降级。
> **WS-relay**(代理作 WS 服务端、把请求隧道给持会话的客户端,如 Gemini AI Studio 那种)是**另一套入站子系统**,
> 不属于 `UpstreamClient`;它持长连接、打破实例无状态、edge 扛不住 → **native-only 的可选子系统,本期不做**(见 §17)。

### 7.5 入站 HTTP:不自造抽象,靠 `tower::Service`

**出站(`UpstreamClient`)抽象成 trait;入站不另立 trait。** axum 的 `Router`
本身就是 `tower::Service<http::Request, http::Response>`——这就是现成的 seam。
约束只有一条:**"构建 Router" 与 "怎么 serve" 分离**(`http::router(state)` 返回
Router;`main` 才 `axum::serve(listener, router)`)。于是 native 用 `axum::serve`
驱动、edge 用平台 fetch 适配器驱动**同一个 Router**,无需 `HttpServer` trait(与 §13
"不搞通用可换宿主层"一致)。注意:edge 构建下 Router/handler 需满足 `?Send`(见 §13)。

### 7.6 构建特性(Cargo features)—— 生产可裁剪

后端/传输按 **Cargo feature** 分,生产只编译用到的,**重依赖(`redis`/`sea-orm`/`wreq`)设 `optional`**,
由 feature 拉起。trait(`CacheBackend`/`PersistenceBackend`/`UpstreamClient`)与 `config` 枚举**不 gate**;
只 gate 后端实现模块 + main.rs 装配(feature 关时对应分支返回运行时错误)。

- 缓存:`cache-memory` / `cache-redis`;持久化:`persist-file` / `persist-db`;出站:`upstream-wreq`
- 边缘(wasm,code-only gate):`cache-libsql` / `cache-upstash` / `persist-libsql` / `upstream-fetch`;
  聚合 `edge`(边缘入口 `http::edge` 需整套 edge bundle)
- `default = [cache-memory, persist-file, upstream-wreq]`(native 零额外重依赖基线)
- 单机生产 = `cargo build --release`(默认即精简,**不含 redis/sea-orm**);
  多实例 = `--features cache-redis,persist-db`;
  开发 = `--all-features` 或 `--features full`;
  边缘 = `--lib --no-default-features --features edge --target wasm32-...`

## 8. 数据模型(逻辑记录)

v2 是**逻辑数据模型**:`db` 实现用 SeaORM 表实现它(全新 schema,**不考虑 v1 迁移兼容**),
`file` 实现用本地文件实现同一份逻辑数据。下列即逻辑记录(`PK=id i64`、
`created_at/updated_at` 默认有,不再重复)。

**A. 路由 / 模型**
- `routes`:`name`(唯一)· `strategy`(weighted/round_robin/failover/least_latency)· `enabled` · `description?`
- `route_members`:`route_id` · `provider_id` · `upstream_model_id` · `weight` · `tier` · `enabled`
- `aliases`:`alias`(唯一)· `route_id`(多对一)
- `provider_models`:`provider_id` · `model_id` · `display_name?` · `pricing_json?` · `variants_json?` · `enabled`
  - **手动模型行**:`model_id` 可以是上游真实 id,也可以是凭空新增的对外 id(不要求上游存在)。
  - **变体(一变多)**:`variants_json` = 纯后缀数组 `["-thinking","-32k"]`(base 照常暴露)或对象
    `{expose_base: bool, suffixes: [..]}`(`expose_base=false` 时只暴露变体、隐藏 base)。
    列表时展开为 `{model_id}{suffix}`;请求侧整名未命中时剥已知变体后缀映射回 base 作
    `upstream_model_id`;变体绑参数注入**不另造机制**——用 process 规则的
    `filter_model_pattern` 匹配**剥离前的原始全名**(如 `*-thinking` + rewrite)。
  - **关联删除**:变体存于行上,删行 = base + 全部变体一并下线,无悬挂。

**B. 供应商 / 凭证**
- `providers`:`name`(唯一)· `channel` · `label?` · `settings_json`(base_url、各 channel 标量开关、**熔断阈值**`circuit_breaker?`{连续失败数或错误率 + 冷却时长},见 §3.2/§7.4)· `credential_strategy`(`round_robin`/`sticky`,sticky key 见 §3.3)· `proxy_url?`(provider 默认出站代理,credential 覆盖 / 全局兜底,见 §7.4)· `tls_fingerprint?`(**JSON**,provider 默认 TLS 伪装指纹,credential 覆盖,见 §7.4)· `enabled` —— **不再有任何 rules 的 JSON 列**,全部提成下列独立表
- `credentials`:`provider_id` · `name?` · `kind` · `secret_json`(**信封加密**,存 `{kek_id, wrapped_dek, nonce, ciphertext}`,见 §14.1)· `weight`(凭证池)· `rpm_limit?` · `tpm_limit?`(凭证级上游速率预算,空=不限;热路径用缓存计数,达额即跳过该 key,主动遵守上游每-key 限额)· `proxy_url?`(覆盖 provider 默认 / 全局出站代理,见 §7.4;edge 忽略)· `tls_fingerprint?`(**JSON**,凭证级 TLS 伪装覆盖,缺省回退 provider 默认,见 §7.4)· `enabled`
- `credential_statuses`:`credential_id` · `channel` · `health_kind` · `health_json?` · `checked_at?` · `last_error?` *(审计快照)*

**B2. 供应商级规则**
- `routing_rules`(**transform-dispatch 决策,非请求改写,仍按 provider 配置**;含 `provider_id` · `sort_order` · `enabled`):`operation` · `kind`(入站 wire kind;内容生成是 `open_ai_responses`/`open_ai_chat_completions`/`claude_messages`/`gemini_generate_content`,非内容生成是 `open_ai`/`claude`/`gemini`)· `implementation`(passthrough/transform_to/local/unsupported)· `dest_operation?` · `dest_kind?` — 唯一约束 `(provider_id, operation, kind)`

请求改写规则(system_text / cache_breakpoint / rewrite / sanitize / header)**不再 provider-scoped**,而是归入**可复用的 rule-set 模型**,再通过 `provider_rule_sets` 挂到 provider:
- `rule_sets`(可复用的命名规则集):`name`(唯一)· `enabled` · `description?`
- `rules`(规则集内的单条规则;含 `rule_set_id` · `sort_order` · `enabled`):`kind`(`system_text`/`cache_breakpoint`/`rewrite`/`sanitize`/`header`)· `config_json`(按 `kind` 存各自字段:`rewrite`={path,action,value_json?}、`sanitize`={pattern,replacement}、`cache_breakpoint`={target,position,index,ttl}、`header`={name,value,mode?:`override`|`merge`,默认 override;merge=逗号合并去重,适用 anthropic-beta 类列表头}、`system_text`={text,position?:`prepend`|`append`,默认 prepend;四种内容生成形态均支持——claude `system`、chat `messages` 系统消息、responses `instructions`、gemini `systemInstruction`;单字段协议按文本拼接,数组协议按块/消息插入};校验下放到 process 层)· `filter_model_pattern?` · `filter_operation_keys?`
- `provider_rule_sets`(M:N 挂载;含 `provider_id` · `rule_set_id` · `sort_order` · `enabled`):把一个 `rule_set` 附到 provider,按 `sort_order` 生效;删 provider 只摘挂载,不删共享的 `rule_sets`

**C. 组织 / 用户 / 鉴权 / 权限 / 限额**

层级:`org → team → user`(多租户)。一个 user 属于一个 org、可选属于一个 team(单归属;多团队成员制留作后续增量)。**权限、配额、限流均可挂在 org / team / user 三级**,统一用 `scope`(`org`/`team`/`user`)+ `scope_id` 表达,避免每级一套表。

- `orgs`:`name`(唯一)· `enabled` · `description?`
- `teams`:`org_id` · `name` · `enabled` — 唯一 `(org_id, name)`
- `users`:`name`(唯一)· `org_id`(FK)· `team_id?`(FK)· `password?`(hash)· `enabled` · `is_admin`
- `user_keys`:`user_id` · `api_key_ciphertext` · `api_key_digest`(唯一索引)· `label?` · `enabled` *(凭证始终系在 user 身份上)*
- `route_permissions`:`scope` · `scope_id` · `route_pattern`(glob,作用在 route 名上)— 用户**有效权限 = 自身 ∪ team ∪ org 的并集**(scoped 模式下 pattern 作用在 **provider 名**上;任何一级都无匹配行 = **默认拒绝**)
- `rate_limits`:`scope` · `scope_id` · `route_pattern` · `rpm?` · `rpd?` · `total_tokens?` — 三级**逐级预检**,任一级超限即拒
- `quotas`:`scope` · `scope_id`(唯一 `(scope, scope_id)`)· `quota_total` · `cost_used`(对账后持久值)— 每请求成本**同时累加到 user/team/org 三级**;预检三级均需通过(最严格者拦截)

**D. 用量 / 日志(只持久化)**
- `usages`(明细,append):`request_id` · `at` · `route_name?` · `provider_id?` · `credential_id?` · `org_id?` · `team_id?` · `user_id?` · `user_key_id?` · `operation` · `kind` · `model?` · `input/output_tokens` · `cache_read/creation_tokens`(+5min/1h)· `cost`
- `usage_rollups`(看板源):`granularity`(hour/day/week/month)· `bucket_start` · 维度(`provider_id?` / `org_id?` / `team_id?` / `user_id?` / `route_name?` / `model?`)· 指标(`requests` / `input_tokens` / `output_tokens` / `cost`)。每请求 `add_usage_rollup` 累加
- `downstream_requests` / `upstream_requests`:抓包日志(受 enable 开关),沿用 v1 结构(下行 path/query,上行 url/latency),各含 `request_id` 串联三处日志。正文按 §14.3 脱敏

**E. 设置(启动)**
- **无配置文件**:启动**只靠环境变量 + CLI 参数**(clap `env=`,每参数同时读 env)。无 `gproxy.toml`。
- Bootstrap 参数(连持久层之前就要):
  - `--persistence <file|db>` / `GPROXY_PERSISTENCE`(默认 `file`)—— 选持久化后端
  - `--data-dir <path>` / `GPROXY_DATA_DIR`(`file` 用,默认 `./data`)
  - `--dsn <url>` / `GPROXY_DSN`(`db` 用;`persistence=db` 时必填)
  - `--redis-url <url>` / `GPROXY_REDIS_URL`(给定即 redis 缓存,否则 memory)
  - `--upstream-proxy-url <url>` / `GPROXY_UPSTREAM_PROXY_URL`(native 出站
    upstream proxy;可选)
  - `--host` / `GPROXY_HOST`(默认 `127.0.0.1`)· `--port` / `GPROXY_PORT`(默认 `8787`)
  - `--instance-name` / `GPROXY_INSTANCE_NAME`(默认 `default`)
  - `GPROXY_MASTER_KEY`(本地 KEK,base64 32B;凭证信封加密用,见 §14.1)
  - `--admin-user` / `GPROXY_ADMIN_USER`(默认 `admin`)· `--admin-password` /
    `GPROXY_ADMIN_PASSWORD?`(admin 引导/密码找回,见 §14.2;密码**推荐用 env 而非
    CLI 旗标**——cmdline 对同宿主其他用户可见且进 shell history)
  - `OTEL_EXPORTER_OTLP_ENDPOINT?`(可选,给定即开启 trace 导出,见 §15)
  - `GPROXY_CORS_ORIGINS?` · `GPROXY_MAX_BODY_BYTES?` · `GPROXY_MAX_INFLIGHT?` · `GPROXY_ADMIN_IP_ALLOWLIST?` · `GPROXY_TRUSTED_PROXY_CIDRS?`(入站防护,见 §16;命名最终以实现为准)
- `instance_settings`(运行时可改,存持久层,按 `instance_name` 每实例一行):`proxy?` · `spoof_emulation?` · `enable_usage` · `enable_upstream_log(_body)` · `enable_downstream_log(_body)` · `disable_log_redaction?`(默认 false,debug 排障用,关脱敏时日志打告警,见 §14.3)· `update_channel?`(自更新通道 `releases|staging`,见 §19)· `update_policy?`(自更新策略 `off|notify|manual|auto`,见 §19)。host/port/dsn/redis/master_key 是 bootstrap,不进此表。

> **不做 Files API 代理**:Operation 体系不含文件管理操作;内联文件引用(content 里的
> `file_id` / image source / document source)照常透传,provider file_id 在 scoped 模式下原样转发。
> 若将来要代理 provider Files API,再连同文件 operation + `files`/`file_permissions` 表一次性补齐
> (届时按"file_id→(provider,credential) 映射"优先设计)。

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
- **plugin 系统 + 可嵌入 SDK 是既定未来方向,但本期不做**:先把单 crate 做扎实,**不预埋
  plugin/SDK 接缝**(过早抽象拖累核心);待 crate 稳定后再作为干净加法引入(自定义 provider /
  translator / 鉴权插件等)。

## 13. 边缘 / WASM 支持

**已上线验证(wasm 跑在真实边缘,服务真 router + 真 Turso/Upstash)**:
**Supabase Edge ✅**、**Netlify Edge ✅**、**Vercel Edge ✅**、**Cloudflare Workers ✅**、**Tencent EdgeOne Pages ✅**、**Deno Deploy ✅**。

**edge 目标平台(V8 isolate / WASM,统一 WinterCG Web Fetch 入口,核心一份 `wasm32` + 每平台薄 glue)**:
Supabase、Netlify、Vercel、Cloudflare Workers、Tencent EdgeOne Pages、Deno Deploy
(已上线);阿里云 ESA(本地 runtime 最小 WASM 可跑,远端默认域名/route 仍未打通到函数,见
`deploy/esa/NOTES.md`)。

EdgeOne Pages 约束:需要 release size profile + 内联 lazy wasm loader
(`__gproxy_load()`) + 显式 route 文件;根 `[[default]].js` catch-all 在直接上传
包中会退回静态资源。详见 `deploy/eopages/NOTES.md`。

**wasm 打包分两路**(实测):Deno 族(Deno/Netlify/Supabase/EdgeOne Pages)= 内联
base64 + 运行时 `WebAssembly.instantiate(bytes)`(EdgeOne Pages 必须 lazy load);
Vercel/Cloudflare =
`wasm-bindgen --target web` + 静态 wasm module import(Vercel/Cloudflare 禁运行时
字节实例化)。

**原生容器平台(Cloud Run / AWS Lambda / Render / Zeabur 等)不在本设计内** ——
它们跑完整 native 二进制(Dockerfile,wreq 伪装全功能),**延到所有功能设计完成后最后再评估/跑**。
(Koyeb / Fly 不做。)

**功能差异(按渠道能力自动降级,见 [§7.4](#74-上游传输抽象-upstreamclient))**:
- `chatgpt`:**边缘不支持**(请求刚需 TLS 伪装),仅传统常驻部署可用。
- `claudecode`:边缘可用,但**仅 token 模式**(需用户自行完成 OAuth 提供 token;
  cookie→oauth 自动换取需伪装,边缘不可用)。
- codex / 各 API-key 类:边缘全可用。

**为边缘需要做的(大多是加实现,非重写)**:
1. `UpstreamClient` 的 `fetch` 实现 ✅(已做:`http/client/fetch.rs`)。
2. **edge 存储(已做,compile-verified)**:
   - **PersistenceBackend = libSQL/Turso over HTTP**(Hrana-over-HTTP via fetch;`store/libsql/`)。SQL,SQLite 方言,和 native `db`(SeaORM)schema 同方言。**不用 KV 做持久化**——KV 做不了 rollup 聚合/即席查询。
   - **CacheBackend = 可插拔,Redis 可选**:`upstash`(Upstash Redis REST,要 Redis)或 `libsql`(同一个 Turso 库的 kv 表,**不要 Redis**)。部署时按配置选,与 native 的 `memory/redis` 对称。
   - **edge 多实例**:isolate 天然多实例;共享状态(限流/quota/session)走上面的共享存储(libSQL 原子 `UPDATE` 或 Upstash 原子 `INCR`);**pub/sub 失效在 edge 不需要**——isolate 朝生暮死、频繁重读配置。
3. wasm 构建 + 各平台 fetch 入口适配器(薄)— Supabase/Netlify/Vercel/Cloudflare/EdgeOne Pages/Deno Deploy 已上线验证;ESA 远端访问链路待打通。
4. `Send` 边界:wasm 上 `#[cfg_attr(target_arch="wasm32", async_trait(?Send))]` ✅(已应用到 CacheBackend/PersistenceBackend/UpstreamClient)。
5. wasm AppState + edge 入口驱动 pipeline — **✅ 落地(2026-06-11)**。`http::edge::init` 从 libSQL 持久化
   **建真控制面快照**(`ControlPlaneSnapshot::build`)+ 接 master_key(`cipher_from_master_key`,空则明文);
   `fetch` **按路径直接派发**到 `pipeline::execute`(网关)/ `metrics::render`(/metrics)/ health,
   **绕过 axum router**(axum `Handler` 要求 `Send`,wasm 网关路径不满足)——native 用 axum handler、edge 用
   fetch 直调,两个适配器共享同一 pipeline 核心。**libSQL 持久化层全量实装**(~40 op 手写 SQLite SQL over
   Hrana,镜像 `db/ops` 语义;`ensure_schema` 自建 25 表;Hrana typed-value 行解码)。
   **注意**:`init` 新增可选 `master_key` 第 5 参 → 各平台 `deploy/*` 的 wasm-bindgen 生成绑定(`gproxy.js`/
   `.d.ts`)需**随发布重新生成**(它们是 wasm 产物快照,现为 4 参旧版)。

**关键限制(edge)**:V8/fetch 无 raw TCP → SQL 直连不可能 → 只能 HTTP-DB(libSQL/Turso)。`base64`/`Instant` 等在 wasm 上自处理(用 `js_sys::Date`,手写 base64)。ESA 本地 runtime 最小 WASM 已验证,但远端默认域名/route 仍未成功进入函数;EO Pages 已验证但有上面的打包/路由约束。

**仍然不做的**:不搞通用"可换 HTTP 宿主"抽象层——native 用 axum、edge 用 fetch 入口,
是**两个具体适配器**(cfg 分目标),不是一层抽象税。核心
(`pipeline / protocol / route / balance / backends`)不依赖 axum。

**节奏**:native 主线优先;edge 已打通存储层(compile),后续做 wasm AppState + 入口 + 部署产物。

## 14. 安全

> 企业级横切关切。本节决策已锁定;实现按 phase 推进(见 §10),抽象先立、云接入作干净加法。

### 14.1 凭证密钥——信封加密
- 抽象 `SecretCipher`(`seal(plaintext) -> sealed` / `open(sealed) -> plaintext`)。
- **信封加密**:每条密钥随机生成 **DEK**,用 AEAD(`AES-256-GCM` 或 `XChaCha20-Poly1305`,
  RustCrypto,wasm 可编译)加密正文;**KEK** 包住 DEK。
- KEK 由 `Kms` trait 提供:**默认本地实现 = env `GPROXY_MASTER_KEY` 作 KEK**;云 KMS
  (AWS/GCP KMS、Vault)作后续 trait 实现,是干净加法。
- `credentials.secret_json` 存信封结构 `{kek_id, wrapped_dek, nonce, ciphertext}`;`kek_id`
  即密钥版本,支持轮换(惰性重封 / 批量重封)。
- 域代码只调 `SecretCipher`,永不直接碰算法 / KMS SDK。
- **算法已定(2026-06-10)**:XChaCha20-Poly1305;`kek_id = local-{blake3(kek)[..8]}`;无 master key = 明文模式(启动告警);信封按形状识别,旧明文行始终可读。

### 14.2 管理端鉴权
- 口令 hash:**argon2id**(RustCrypto,wasm 兼容)。
- 会话:**服务端不透明 session token 存 cache**(可吊销,带 TTL),优于 JWT(免吊销难题);
  会话状态走 §7.2 的 redis(native)/ libsql|upstash(edge)。
- Console 用 httpOnly + Secure + SameSite cookie 携带 session。
- 用户 API key 仍走 `user_keys` 的 `api_key_digest`(不变);管理端登录是独立通道。
- **首启 admin 引导(唯一内置引导)**:启动完成持久层健康检查与首启导入(§18,若有)后,
  若 `users` 表为空 → 自动创建默认组织(`default`)与用户 `admin`(`is_admin=true`,挂该 org 下):
  密码 CSPRNG 随机生成(URL-safe,≥24 字符),argon2id 哈希入库,**明文只打印一次**到启动
  stdout/日志(醒目框出,提示登录后立即修改);明文不落任何持久层/缓存。非空库永不触发。
  与"无 seed、无内置 provider 模板"不冲突——那禁的是业务配置种子,这里只解决
  "空库无人能登录管理面"的自举问题。native-only;edge 的引导随其导入路径另定(§18)。
- **admin 凭证覆盖(密码找回)**:`--admin-user` / `GPROXY_ADMIN_USER`(默认 `admin`)+
  `--admin-password` / `GPROXY_ADMIN_PASSWORD`(§8-E)。设了密码 → 每次启动在引导点对该
  用户**强制 upsert**:不存在则按首启路径创建(用给定密码,不再随机生成+打印);已存在则
  重置 password hash 并强制 `enabled=true`、`is_admin=true`(宿主级恢复动作,语义即
  "夺回管理权")。生效期间每次启动打**醒目告警**(凭证来自 env/CLI,恢复后应移除);
  密码值永不写日志/配置 dump。覆盖在首启导入(§18)之后应用,对导入带来的用户同样生效。
  信任模型与 §18 同源:能设进程 env/CLI 即已有宿主访问权,而宿主上本就有
  `GPROXY_MASTER_KEY`(可解全部凭证)与数据目录/DSN(可直改 users 表),此机制不扩大攻击面;
  唯 CLI 旗标在 `/proc/*/cmdline` 对同宿主其他用户可见 → 密码**推荐走 env**。
- **首启引导 + 凭证覆盖落地(2026-06-11)**:`app/bootstrap.rs::ensure_admin` 实现上述两条,
  在首启导入(§18)之后、构建快照之前调用(导入带来的 admin 抢占随机创建)。空库 + 无覆盖 →
  建 `default` org + 随机口令 `admin`(CSPRNG ≥24 字符,argon2id 入库,只 `println!` 打印一次,
  明文不落库/日志);设了 `--admin-password` → 每次启动强制 upsert 该用户(`enabled`/`is_admin`
  置真),并打告警(告警不含密码);非空库 + 无覆盖 = no-op,重复启动不产生第二个 admin 或第二个
  `default` org。`crypto::password::{hash,verify}`(argon2id,salt 取自 `util::rand`,跨目标含
  wasm edge)就绪供 M10 登录。native main-path。
- **会话鉴权 + admin 登录落地(M10a,2026-06-11)**:`admin/session.rs` 实现不透明
  session token——32 字节 CSPRNG(`util::rand`)→ url-safe base64,cache 存
  `sess:{token}` = user_id(可吊销,12h 滑动 TTL,每次成功 `validate` 续期)。
  `validate` **每请求回读持久层并校验 `enabled` + `is_admin`**(中途禁用/降权立即生效),
  `revoke` 删除条目。`http/server/admin/`:`require_admin` 中间件(cookie → 会话 →
  注入 `AdminUser`,否则 401)+ `/admin/login | /admin/logout | /admin/me`。
  cookie 始终 `HttpOnly; SameSite=Lax; Path=/; Max-Age=<ttl>`,`Secure` 默认开,
  仅 `GPROXY_INSECURE_COOKIES=1`(本地明文 HTTP 开发)关闭。登录任何失败路径返回**同一
  通用 401**(无用户枚举),`crypto::password::verify` 校验,口令/ token 不落日志。
  login/logout 公开,其余 `/admin/*` 在中间件之后(无自锁)。token 不透明、非 JWT,
  故吊销即时。
- **配置 CRUD 落地(M10b,2026-06-11)**:`http/server/admin/crud/` 提供全量配置 CRUD
  (全局 + 父级嵌套 + 含密 + authz 作用域),全部在 `require_admin` 之后。**读侧脱敏**
  ——secret/password hash/api_key 密文永不出网,仅返回 `has_*` 标志
  (`CredentialView` / `UserKeyView` / `UserView`;明文导出是唯一例外路径);**写侧密封**。
  每次 mutation(upsert/delete)调用 `admin::invalidate` 广播 `INVALIDATE` 并重载本地
  快照(收口 M8 admin-write 失效)。

### 14.3 日志正文脱敏
- 开 `enable_*_log_body` 抓正文时,**强制剥离 Authorization / api-key 等密钥头与已知密钥字段**
  (不可配,默认就做)。本期**不做 PII 正则**(邮箱/卡号),留作以后可选。
- **debug 可关**:`instance_settings.disable_log_redaction`(默认 false);关闭脱敏时每条日志打
  **醒目告警**,提示正文含敏感信息。

### 14.4 入站 TLS 边界
- native **只服务 HTTP**,TLS 由**前置 LB / ingress 终结**;edge 由平台终结。
- native 内置 rustls(直服务 HTTPS)以后再评估,本期不做。

### 14.5 凭证获取与 OAuth 刷新

订阅账号(ChatGPT/Claude/Gemini 等)经 OAuth 接入,凭证生命周期是这类代理的立身之本。

**获取(登录)——回传 callback URL(2026-06-11:只保留这一种)**:
- 两步走 admin 操作:① `start`:服务端生成 **PKCE verifier + state**,存 cache(短 TTL,keyed by login-session),返回 auth URL;② `complete`:operator 在浏览器完成授权后,把跳转后的**整条 callback URL**(含 code+state)贴回,服务端用存的 verifier 换 token 入库。
- **为什么只走回传 URL**:不建本地回调服务器、不自动开浏览器——那套假设代理跑在有浏览器、localhost 可达的机器上;远程 VPS / 容器 / **边缘 serverless** 上不成立。回传 URL 流程**到处可用、天然 edge 兼容**(纯 HTTP 两步)、攻击面最小。
- **device flow**:不走 callback redirect 的 provider 另用 device flow(显示 code + 轮询)—— copilot **强制**(GitHub device flow);codex 支持(`headless_chatgpt_login`,远程/headless 机器用);kiro 端点支持则可选。
- **逐渠道登录机制(2026-06-11 核实 v1+sample;HTTP 入口 M10)**:
  | 渠道 | 登录机制 |
  |---|---|
  | 全部 | **直接导入 token**(粘已有 access/refresh token,M9 导入天然支持——也是 M7b 测试方式) |
  | vertex | service-account JSON(无 OAuth) |
  | geminicli / antigravity | OAuth authcode(粘 callback URL)+ project 解析(loadCodeAssist/onboardUser) |
  | codex | OAuth authcode(粘 callback URL)**+ device code**(headless) |
  | copilotcli | **device code**(GitHub 强制) |
  | claudecode | OAuth authcode(粘 callback URL)**+ cookie 粘贴**(sessionKey → claude.ai org 发现 → authorize → 换 token) |
  | kiro | OAuth authcode(社交 portal / AWS IdC,粘 callback URL) |
- **登录代码归属**:渠道知识(client_id / 授权 + token 端点 / PKCE 配置 / code↔token 换取 / device 轮询 / cookie 换取)在**各 channel**(可单测);登录编排(start/complete 状态机)是共享服务;实际 HTTP 入口端点在 **M10 管理 API**。M7b 用导入预置 token 的凭证测试 prepare+刷新+转换,不依赖 M10。

**M10c 登录端点落地(2026-06-11)——获取 token 这半边已完成**:`ChannelLogin` trait(默认 unsupported 的 `authcode_start`/`authcode_exchange` + `device_start`/`device_poll` + `cookie_exchange`)+ `login_for(channel)` 注册表;`require_admin` 后的 admin `login-flows` 端点:`start`/`complete`(authcode,PKCE+state 存 cache、`complete` 校验 state)、`device/start`/`device/poll`(device-code,session 一次性存 cache、pending 时 peek 不删、终态才 clear)、`cookie`(一步换取)。按上表逐渠道:authcode = codex/claudecode/geminicli/antigravity/kiro-social;device = copilotcli(GitHub device flow → `{github_token}`,refresh 再换 Copilot token);cookie = claudecode。成功即 seal+建 `kind="oauth"` 凭证并广播失效,返回 **脱敏的 `CredentialView`**(绝不回传 secret;verifier/device_code/cookie/code 全程不记日志)。codex device **跳过**——codex sample 走自有 app-server RPC(不可干净提取的公开 device 端点),authcode 已覆盖。后续:kiro AWS IdC(OIDC `RegisterClient`)、geminicli/antigravity project 解析、codex `account_id`(从 `id_token`)。

**刷新——惰性按需 + 单飞锁**:
- secret_json(信封加密)存 `{access_token, refresh_token, expires_at, ...}`。
- 选中凭证时若 `now > expires_at - lead`(按 provider 设提前量)或上游返 **401** → 先刷新再用/重试。
- **⚠️ 必须按凭证单飞**:许多 provider **每次刷新轮换 refresh_token**(旧的即失效);多实例/并发刷同一凭证会让落败方拿到作废 token → 凭证掉线。单实例用本地 mutex,**多实例用 redis 分布式锁**(锁 key = credential id)。
- 刷新成功 → 新 token **用目标 KEK 重新信封加密写回 persistence** + `cache.publish` 广播失效(见 §7.2/§14.1);失败 → 标 credential `status=error` + 退避 + 上报 admin。
- 可选:轻量后台预刷(持锁的某实例扫临近过期的凭证提前刷)消除"过期后首请求"的延迟尖刺。

**M7a 落地(2026-06-11)**:惰性 + 单飞(本地 mutex,redis 分布式锁 = M8 seam)已接入 failover;AuthDead 触发强制刷新 + 同候选重试一次。

**M8 跨实例刷新锁落地(2026-06-11)**:M7a 的 redis 分布式锁 seam 已接通。`ensure_fresh` 先取
**本地 per-credential mutex**(单实例单飞),再取 redis 锁 `gproxy:refresh:lock:{cred id}`
(`SET NX PX`,默认在 memory/edge 上恒为 `true`,故单实例/wasm 走快路径)。锁**只环绕实际上游
refresh 调用**:刷新返回后立即在所有出口(含错误 `?` 前)`unlock`,**绝不跨 seal/写回/publish 持锁**。
落败实例短暂等待后重读凭证,若赢家已轮换则复用其结果,避免二次轮换烧毁单次性 refresh_token。
锁短 TTL 兜底卡死;token 化的 check-and-delete unlock 是后续硬化项。

**vertex 边缘签名(2026-06-11 落地)**:vertex 的 SA-JWT(RS256)曾 native-only,**不是** Google/协议限制,是 `jsonwebtoken` v9 只有 `ring` 后端、不能编译到 wasm32。改用 **jsonwebtoken v10 + `rust_crypto` feature**(内部用 RustCrypto 的纯 Rust `rsa`/`sha2`,无 ring),vertex 即**全平台(含 edge)签名**;保留 jsonwebtoken 干净 API。RNG 统一到 `util::rand`(getrandom,跨目标),不再依赖 cipher crate re-export。token 一小时签一次,性能无关。配套:chacha20poly1305 0.11(crypto-common 0.2)在 wasm 上需 getrandom 0.4 的 `wasm_js` backend + `.cargo/config.toml` 的 `--cfg getrandom_backend` rustflag。

## 15. 可观测性

> **落地(2026-06-11)**:§15.1 ULID request_id ✅、§15.2 每请求 tracing span ✅、§15.3 持久化派生
> `/metrics`(native + edge)✅;OTLP 导出仍留 env 接缝待真 collector;in-flight gauge 砍掉(见 §15.3)。

### 15.1 request_id(请求关联 ID)
- 入站即生成 **ULID** `request_id`(`util::id::ulid()`,自渲染 Crockford base32 = 48-bit 毫秒 + 80-bit 随机,
  复用单一 RNG 源 + 双目标时钟,wasm 安全),贯穿整个 pipeline、tracing span、全部日志。
- **写入 `usages` / `downstream_requests` / `upstream_requests`**(§8 已加列),串联三处日志。
- 响应回 `x-gproxy-request-id`;客户端带来的 `x-request-id` **单独记录、不直接信任**(自身始终另生成)。

### 15.2 tracing
- 结构化 `tracing`:每请求一根 span(`pipeline::execute` 入口建),带 `request_id`,classify/route 后
  补 `op` / `kind` / `route` / `provider`。
- **OTLP 导出可选**(env `OTEL_EXPORTER_OTLP_ENDPOINT`,给定即开)——**留 env 接缝、暂未接**(等真 collector
  才能验证;现本地 fmt 日志 + request_id 串联够用)。OTLP-over-HTTP 在 edge 经 fetch 可用。

### 15.3 metrics
- **Prometheus `/metrics`** 文本端点,**完全持久化派生、零内存计数**:从 `usage_rollups`(hour 粒度,
  请求/token 总量)、`usages.latency_ms`(上游延迟直方图,`>0` 的成功+失败)、`credential_statuses`
  (按 health_kind 计数)、`quotas`(用量 gauge)用**聚合 SQL**(SUM/COUNT/CASE 分桶)算出,handler 手渲染
  Prometheus 文本(`render`)。
- **native + edge 通用**:同一 `render` + `metrics_aggregate` trait op(db / file / libsql 三后端各实装);
  native 走 axum `/metrics` 路由,edge 走 fetch 直接派发。**多实例天然全局聚合**(状态在库不在进程)。
- **访问控制(2026-06-12)**:`/healthz`、`/version` 与 `/metrics` 复用 `/admin/*` 的管理鉴权
  (`admin::authenticate_admin`:admin session cookie **或** admin 用户的 API key——后者供
  curl / Prometheus 等无头客户端,仅认 header 形态、不认 `?key=`;无有效凭证一律 401,
  **无任何公开 ops 端点**);edge 侧走同一函数(session 由共享 cache+persistence 的 native
  实例签发,key 走 snapshot,与网关鉴权同源)。
- **in-flight gauge 不做**:瞬时"在途请求数"本质只能来自进程内存,与"零内存"决策冲突 → 砍掉。
- 上游延迟覆盖成功请求:结算路径(`settle`)把成功尝试的 **TTFB**(失败器测得的 `send_ms`)写入
  `usages.latency_ms`(A4),直方图据此非空。

## 16. 韧性与入站防护

### 16.1 优雅停机 / 排空
- SIGTERM/SIGINT:停收新请求 → 限时排空 in-flight → 退出(`axum::serve().with_graceful_shutdown`,
  tokio signal 已在依赖)。**native-only**;edge 生命周期归平台。

### 16.2 过载保护
- 全局**最大在途**上限 + load-shed → 503(tower 层)。per-provider 并发上限留作 provider 设置以后加。

### 16.3 健康探测
- **只被动**:失败冷却 + member/凭证熔断(§3.3);**不做主动周期探活**(避免对 LLM 上游白烧钱)。
- **状态可见性(2026-06-10 定)**:热路径健康/熔断/冷却是**每实例内存软状态**(决策只读内存,
  每次失败计数不落库);但**状态变迁边沿**——进入/退出熔断、进入 429 冷却、半开失败再冷却、
  标记 error——**异步 fire-and-forget 落 `credential_statuses`**,`health_json` 携带
  `{state, open_until, consecutive_failures, reason}`,给管理面"何时冷却、冷却到何时"的
  历史与近实时视图(写失败不影响请求;多实例各写各的,行内带 instance 标识)。
  秒级精确的当前态由管理 API 直接读实例内存(M10;跨实例聚合归 M8/M10)。
  熔断半开:同一时刻只放一个探测请求;连续失败冷却时长指数退避、封顶。

### 16.4 入站防护
- **CORS**(可配允许源,给 Console/浏览器)。
- **请求体大小上限**(可配;LLM 请求大,默认放宽但有界)。
- **管理端 IP allowlist**;`X-Forwarded-For` 仅信任可配的前置代理 CIDR。
- 配置项见 §8-E 的 `GPROXY_CORS_ORIGINS` / `GPROXY_MAX_BODY_BYTES` / `GPROXY_MAX_INFLIGHT` /
  `GPROXY_ADMIN_IP_ALLOWLIST` / `GPROXY_TRUSTED_PROXY_CIDRS`。

## 17. 计费与配额(幂等)

- **幂等键 = `request_id`**:同一请求只计一次;failover 的失败尝试**不计费**,只对**成功那次**按其真实
  usage 计。`usages` append 与 rollup 累加均按 request_id 幂等。
- **配额先扣 → 对账**:预检阶段先扣**估算**(§7.2"cache 先扣"),响应后按实际 usage **对账多退少补**。
- **流式预扣 = 消息总长度 ×1**(初始 token 估算,全额预扣、对账多退);流结束按最终 usage 事件对账;**非流式按实际**。
- **流式结算细则(2026-06-10 定)**:热路径**零解析**——转发 chunk 时只 push 引用计数的 Bytes
  克隆进**有界缓冲**(约 4MB,超限保尾部+总字节数);结算在**显式结束或 Drop 守卫**触发的
  spawn 任务里离线做,`request_id` 幂等恰好一次。
  - **正常结束**:从缓冲尾部解析出上游最终 usage 照记(openai 流**注入
    `stream_options.include_usage`**,否则拿不到);提取器统一 cache 语义——归一口径
    **input = 非缓存输入,cache_read/creation 单列**(openai `prompt_tokens` 含 cached 需减,
    claude 天然分列),details 缺失 → cache=0、input 全价(只多记不少收)。
  - **异常结束(上游断/客户端断)**:按**已产出部分**计费(客户端断不按预扣全额)。
    计数阶梯:gpt 族 → 本地 tiktoken(精确);claude/gemini 族 → **上游 count 端点**
    (原请求体→input,缓冲提取文本→output;全局并发上限+超时,不走用户配额,失败降级);
    其余/失败 → 本地词库链(→ chars/2)。
  - 落库标记:`usage_source: upstream | counted | estimated` + `ended: complete | interrupted`;
    上游 mid-stream 断时给客户端补**协议形状的错误帧**再收尾,不裸断。
- 多级配额(user/team/org)三级计数器同步累加,见 §7.2 / §8-C;预扣 pending 计数带 **TTL 自愈**
  (进程崩溃后到期自动释放,无需恢复扫描)。

**M8 配额对账原子化落地(2026-06-11)**:结算对账(`pipeline/settle/reconcile.rs`)持久化实际成本时,
`quotas.cost_used` 的累加现为**原子增量** `add_quota_cost(scope, scope_id, delta)`,关闭原 M6 的
read-modify-write 跨实例丢失更新竞态。file backend 在写锁下 load→`+= delta`→store;db backend 因
`cost_used` 是 TEXT 十进制串列(SQL `+` 不可直接做小数运算),改用**单事务内 read-add-write**
(SQLite 串行化事务;PG/MySQL 在事务内行锁选中行)。行缺失即 no-op(该请求不计费)。预扣 pending 退款
仍用原子 `cache.incr`(同额退还、永不重算)。

> **本轮未纳入(留作干净加法)**:管理审计日志(`audit_logs`)、日志/用量数据保留与清理、
> per-provider 并发上限、PII 正文脱敏、云 KMS 实现、native rustls、主动健康探活、
> Files API 代理(及 `files`/`file_permissions` 表)、
> WS-relay 入站隧道子系统(AI Studio 式,native-only、对多实例不友好;WS-out 见 §7.4 仍照建接缝)。

## 18. 配置导出 / 导入(迁移)

无论哪种部署后端(`file`/`db`/`libsql`),都能把**全部控制面配置 + 凭证**导出到**单个文件**,
便于备份与跨部署迁移。它建在 typed `PersistenceBackend` 之上(读出/写回所有实体),**backend 无关**;
**依赖实体层(Gap 2)落地后实现**。

- **范围**:orgs/teams/users/keys、providers/credentials、routes/route_members/aliases/provider_models、
  §8-B2 规则表、`instance_settings`。**不含** usage / 日志 / rollup(运营数据,不属于配置迁移)。
- **格式**:单 JSON,带 **schema 版本号**(跨版本导入时配合迁移);凭证**明文导出**。
- **门 = `GPROXY_MASTER_KEY`,与 admin_key 无关**(能解密 secrets 的是主密钥;admin_key 只是 API 层鉴权):
  - **导出**:需主密钥(把信封解密成明文)。CLI/ops 操作(`gproxy export --out f`),从 env 读主密钥。
  - **导入**:需主密钥(把明文 secrets 用**目标实例 KEK 重新信封加密**后落库)。CLI/首启
    (`gproxy import --in f` 或 `GPROXY_IMPORT_FILE`),靠**宿主访问权**——目标为空库、无 admin,
    天然适合新机首次迁移。
- **安全**:明文文件一旦泄露即全量凭证泄露 → 导出时打**醒目告警**、建议文件权限 600;
  将来可选 `--passphrase` 给整文件再包一层 AEAD(默认仍明文)。
- **edge**:无 CLI/本地文件;edge 的导入路径单独处理(后续)。

**导出落地(2026-06-11)**:`app/export.rs`(+ `export/mappers.rs`,`gproxy export --out`)
读出全部控制面实体、把 secrets 解信封为明文、按 `import::Bundle` 形状回吐,故 `export | import`
往返一致。范围按 §8-E 枚举 scope-universe(orgs+teams+users)拿 quotas/limits/perms(无 `list_quotas`,
逐 scope `get_quota`);每条 `*Input` 保留 `id`,re-import upsert 同一行(交叉引用稳定);导出告警只带
文件路径、不带任何 secret。**FILE + DB backend 均完整往返**(2026-06-11,`37abad6`:DB 的 18 个配置
实体 upsert 在显式 id 未命中时按显式 id 插入,与 file 一致)——`import --persistence=db` 现可给空库做种
(sqlite 已验;Postgres 做种后的自增插入需序列同步,见 `db/ops/mod.rs` 注释,M10/admin 关切)。

## 19. 自更新(发布后,设计先行;暂不实现)

> 状态:**仅设计**(2026-06-11)。发布前无法真实演练(需真实 release 制品/签名密钥/真机),故本节
> 只锁定形态与决策,**不写代码**;实现待首个 release 就绪后单列里程碑。

### 19.1 范围与非目标
- **仅 native 单二进制**(`gproxy` bin)。Console 经 rust-embed 嵌入二进制,**更新二进制即更新 Console**。
- **edge(wasm)不自更新**:Cloudflare Workers / EdgeOne Pages 走平台的 wrangler/deploy 流水线发布,
  自更新逻辑全部 `#[cfg(not(target_arch="wasm32"))]`。
- **数据不动**:配置/凭证在持久层(file/db),二进制里没有业务数据;自更新只换可执行文件,
  持久层是跨版本的真相源。

### 19.2 信任锚(供应链安全,最关键)
- **签名清单**:每个 GitHub Release 附一份**版本清单**(manifest JSON)+ 每平台制品。清单字段:
  `{ channel, version, notes_url, min_compatible_data_version, artifacts: [{ target_triple, url, sha256, size }], signature }`。
- **签名校验不可绕过**:清单用 **ed25519**(minisign 风格)签名;**公钥编译进二进制**(build 期 const)。
  自更新流程:下载制品 → sha256 比对清单 → 清单签名用内嵌公钥验签 → 任一不符即**拒绝**。
  **无有效签名绝不替换二进制**。这是自替换二进制的硬底线(两通道同样适用,staging 也必须验签)。
- 传输走 HTTPS(TLS)+ 签名,纵深防御。`ed25519-dalek` 已在依赖树(jsonwebtoken rust_crypto 带入),
  或用 `minisign-verify`,不引重依赖。

### 19.3 两条发布通道(GitHub Releases)
通道与策略是**正交两维**,各占一个 `instance_settings` 字段:
- `update_channel`:`releases | staging`(默认 `releases`)。
- `update_policy`:`off | notify | manual | auto`(默认 `manual`)。

(§8-B 原预留的单字段 `update_channel` 重新定义为这一维;策略另立 `update_policy`。)

- **`releases` 通道——语义版本**:每次发版打 `vX.X.X` tag、一个新 Release。检查 = 拉
  `https://github.com/{repo}/releases/latest` 的清单 → 比 **semver**(清单 `version` 权威),
  本地 `CARGO_PKG_VERSION` 落后即有更新。给生产用。
- **`staging` 通道——滚动覆盖、按 sha256 判更新**:**固定**一个 tag 为 `staging` 的 Release,
  CI 每次把制品**滚动重传**到它,version 不变(恒为 `staging` 或无意义)。所以**不能比 semver**——
  检查 = 拉 `https://github.com/{repo}/releases/tag/staging` 的清单 → **比清单里本平台制品的 `sha256`
  与本地正在跑的二进制 sha256**,不等即有更新。给尝鲜/灰度用。
- 本地二进制 sha256:启动时算一次 `current_exe()` 的摘要并缓存(staging 比对用;releases 通道不需要)。

### 19.4 版本检查与策略
- 当前标识 = `releases` 看 `CARGO_PKG_VERSION`;`staging` 看本地二进制 sha256(§19.3)。检查经
  `UpstreamClient`(proxy-aware)拉对应通道的清单。
- 触发:① admin API(`GET /admin/update/check`);② 可选低频后台轮询(默认关或按天)。**轮询绝不自动应用**。
- 策略(`update_policy`):默认 **`manual`**(检查+上报,管理员批准才应用);`auto` = 按计划检查+应用+重启
  (opt-in,有风险);`notify` = 仅上报可用性;`off` = 不检查。


### 19.5 下载 + 原子替换
- 下载到 `data_dir/.update/<version>.tmp`(与二进制同文件系统,保证 rename 原子);验 sha256 + 签名;置可执行。
- **原子替换**:Unix —— rename 新文件覆盖当前二进制路径(运行中进程持有旧 inode/text 映射,继续跑;
  新 exec 取到新文件);旧二进制留为 `<path>.prev` 供回滚。Windows —— 运行中 .exe 不可直接替换:
  当前 → `.old`、新 → 当前、延迟清理(或要求 supervisor)。自身路径用 `std::env::current_exe()`。
- 用 `self-replace` crate 抹平 Unix/Windows 差异。

### 19.6 重启 / 交接(两种模型)
1. **Supervisor 重启(推荐默认,容器部署)**:暂存新二进制 → §16.1 优雅停机(排空在途)→ 以哨兵码退出;
   systemd/docker/k8s 的 restart policy 拉起新二进制。重启与滚动归编排器。
2. **自 re-exec(裸部署,"下载即跑")**:优雅排空后 `execv` 用新二进制替换进程映像(尽量继承监听 socket,
   否则重绑)。无 supervisor 的场景用。
- 由设置 / 环境探测二选一。

### 19.7 数据 / schema 迁移
- 新二进制**启动时跑迁移**(幂等 schema create/migrate)。清单带 `min_compatible_data_version`:
  拒绝降级或跨越不兼容跳变。持久层不被替换动作触碰,仅二进制变。
- (正式迁移框架是独立关切;当前 `schema::create_all` 是起点,留作增量。)

### 19.8 回滚
- 保留 `<path>.prev`。重启后新二进制须在超时内过 `/healthz`(开机自检 or supervisor 健康探针);
  失败 → 回退 `.prev` + 重启。supervisor 模型靠其健康探针 + 保留的 `.prev` 做手动/自动回滚。

### 19.9 多实例 / 车队
- 自更新**逐实例**。车队滚动归编排器(k8s rolling / docker)。裸车队可选 **redis "一次一个" 守卫**
  (锁 `update:rolling`,一次只放一个实例排空+更新)。与 M8 配置失效无关——这是二进制更新,不是配置变更。

### 19.10 admin API 面(实现时随 M10 风格落地)
- `GET /admin/update/check` → `{current, latest, available, notes_url}`。
- `POST /admin/update/apply` → 暂存+验签+(按策略)重启。
- `GET /admin/update/status` → `{state: idle|downloading|staged|restarting|failed, ...}`。
- 均在 M10a `require_admin` 之后;`.update` 目录 + 暂存二进制 chmod 700。

### 19.11 待确认(实现前)
- **发布主机**:GitHub Releases vs 自托管清单 URL。
- **签名私钥托管**:私钥存放位置 + 轮换(公钥内嵌,轮换需发版携带新公钥过渡)。
- **默认重启模型**:容器=supervisor;单二进制"下载即跑"=自 re-exec —— 二者都支持,默认值待定。
