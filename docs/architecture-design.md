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
  config/                 # toml 种子解析、运行时配置快照(ArcSwap)、热更新
  http/                   # 所有 HTTP 入口(axum)
    middleware/           # auth / classify / ratelimit / permission / sanitize
    admin/                # 管理后台 API(按资源拆,杜绝上帝文件)
    gateway/              # 代理端点 /v1、/{provider}/v1
    console.rs            # rust-embed 静态资源
  pipeline/               # 请求生命周期编排(替代旧 engine.execute 巨函数)
    classify.rs preprocess.rs route.rs balance.rs retry.rs execute.rs stream.rs
  protocol/               # 线格式类型 + 转换(去臃肿后)
    openai/ claude/ gemini/   # 各方言 wire 类型
    transform/            # 两两转换
      common/             # 收敛后的共享样板(SSE 分帧、role/tool 映射、usage 搬运、错误包装)
      dispatch.rs         # (from, to) → impl 转换表,替代手写巨型 match
  channel/                # 各上游客户端(openai/claude/gemini/codex/...)
  store/                  # 存储抽象
    cache/                # CacheBackend trait + memory + redis 实现
    db/                   # SeaORM 实体 + 查询 + 迁移
    domains/              # 各数据域仓储,声明 缓存/持久化 策略
  auth/ quota/ billing/ ratelimit/   # 横切领域
```

设计原则:**任何一个文件都能单独读懂、单独测试**。文件变大即是"做了太多事"的信号。

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

核心抽象 `CacheBackend`(get / set / incr / expire / cas / pub-sub),两个实现:

- **memory**(单实例,dashmap)
- **redis**(多实例)

启动时按配置选择,业务代码无感。**Redis 可选**——不配置即用 memory。

### 7.1 各数据域策略

| 数据域 | 策略 | 说明 |
|---|---|---|
| 配置 / 供应商 / 路由 / 模型 | 写穿 | DB 真相源,缓存加速;改动经 Redis pub/sub 通知各实例失效重载 |
| 配额(钱) | 写穿(强持久化) | 缓存扣减 + 异步落库,绝不丢账 |
| 限流窗口 | 只缓存 | 瞬时计数,过期即弃,不落库 |
| 凭证健康 / 熔断冷却 | 只缓存 | 运行时状态,重启可重建 |
| 用户登录态 / session | 只缓存 | 不持久化 |
| 请求日志 / 审计 / 用量明细 | 只持久化 | 直接落库,不进缓存 |

### 7.2 多实例语义

- 实例本身**无状态、可水平扩**。
- 共享状态全部经 `CacheBackend`(redis)+ DB。
- 配置变更:写 DB → Redis pub/sub 广播失效 → 各实例重载配置快照(`ArcSwap`)。

## 8. 数据模型(SeaORM 实体重设计)

v2 全新 schema(不背 v1 增量迁移)。负载均衡新增:

- **`routes`**:逻辑模型名、第一层均衡策略枚举、归属(全局)。
- **`route_members`**:`route_id → (provider_id, upstream_model_id, weight, tier, enabled)`。
- **`aliases`**:`alias_name → route_name` 多对一映射(预处理层)。
- 凭证池:沿用 `credentials` + `credential_statuses` 思路,补 `selection_strategy` 字段。
- 其余实体(users / user_keys / permissions / providers / usages / requests 等)按 v1 语义重整命名,清理含糊列。

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
2. **管线 + passthrough**:auth / classify / preprocess / route / balance / execute,先只做同协议 passthrough。
3. **协议转换移植**:trait + common 收敛,逐对迁移转换逻辑。
4. **负载均衡**:两层池 + 熔断 / 凭证冷却。
5. **多实例**:Redis 后端 + 配置 pub/sub 失效。
6. **Console 重做**。
7. **数据迁移**:v1 DB → v2 schema 的一次性迁移脚本。

## 11. 代码规范(实施期强制)

- **文件大小**:单文件理想 **≤200 行**,**绝对不超过 500 行**。确需超过的,先经人工审批。
- **结构化组织**:按职责拆分模块,文件变大即视为"做了太多事"的信号,及时拆分。
- **写前检查**:动手前先搜索代码库是否已有部分实现可复用/扩展,并评估是否需要先做小范围重构,避免重复实现。
- **格式与 lint**:每次写完/改完代码,收尾必跑 `cargo fmt` 与 `cargo clippy`,clippy 告警要修而非压制(除非有明确理由)。
- **测试克制**:**不做默认 TDD,不写大量冗余测试,避免过度测试**。仅对真正棘手的逻辑、真实 bug 的回归点写精简测试。

## 12. 开放问题

- v1 → v2 数据迁移的具体字段映射,待 v2 schema 定稿后单独成文。
- 各 channel 的移植优先级排序(哪些 provider 先迁)。
