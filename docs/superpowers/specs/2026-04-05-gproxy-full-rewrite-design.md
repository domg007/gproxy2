# gproxy 彻底重构计划

## Context

gproxy 是一个 Rust LLM API 代理网关，当前功能完整但架构存在多项严重问题：同步日志写入阻塞请求路径、App/SDK 双写耦合、AppState God Object、ArcSwap 并发写 bug、quota 崩溃丢数据等。Owner 对功能点满意，希望保留全部功能、彻底重构架构，目标是高并发企业级 LLM 网关，单实例优先但为多实例留路。

---

## 一、Crate 重组

### SDK 层（四 crate，其他应用可复用）

| Crate | 职责 | 关键变化 |
|-------|------|---------|
| `gproxy-protocol` | wire types + 跨协议转换 + SSE→NDJSON | 不变 |
| `gproxy-routing` | **新 crate**：classify 纯逻辑 + model_extraction + provider_prefix + suffix groups | 从 `gproxy-server/middleware/` 提取纯逻辑（无 axum 依赖） |
| `gproxy-provider` | Channel trait + 14 渠道 + Engine + ProviderStore + retry + health + billing + affinity | 新增 `RateLimitBackend`/`QuotaBackend`/`AffinityBackend` trait 定义 + 内存默认实现；ProviderStore 改用 DashMap 支持增量更新；engine 构建时注入 backend |
| `gproxy-sdk` | re-export 上面三个 | 不变 |

### App 层（四 crate，gproxy 专属业务）

| Crate | 职责 | 关键变化 |
|-------|------|---------|
| `gproxy-core` | Domain services: IdentityService, PolicyService, QuotaService, FileService, AliasService + Redis backend impls（可选 feature） | **新 crate**，取代当前 AppState God Object |
| `gproxy-storage` | SeaORM Repository pattern，DB 读写 | 移除 `StorageWriteEvent` enum，改为 Repository trait per aggregate root + `WriteSink` for logs |
| `gproxy-api` | axum 路由 + thin middleware wrappers + handler | middleware 只是 SDK routing 纯函数的 axum adapter |
| `apps/gproxy` | 二进制入口 + bootstrap + config + worker spawning | 装配所有 service 和 worker |

### Crate 依赖图

```
gproxy-protocol  (无内部依赖)
       ▲
gproxy-routing → gproxy-protocol
       ▲
gproxy-provider → gproxy-protocol, gproxy-routing
       ▲
gproxy-sdk → (re-export 全部)
       ▲
gproxy-core → gproxy-sdk
       ▲
gproxy-storage → gproxy-core, gproxy-sdk
       ▲
gproxy-api → gproxy-core, gproxy-storage, gproxy-sdk
       ▲
apps/gproxy → gproxy-api, gproxy-core, gproxy-storage, gproxy-sdk
```

严格单向：SDK 对 App 零依赖，无循环。

---

## 二、核心 Trait 抽象（定义在 `gproxy-provider`）

### RateLimitBackend

```rust
pub trait RateLimitBackend: Send + Sync + 'static {
    fn try_acquire(&self, key: &str, window: RateLimitWindow)
        -> impl Future<Output = Result<u64, RateLimitExceeded>> + Send;
    fn current_count(&self, key: &str, window: RateLimitWindow)
        -> impl Future<Output = u64> + Send;
}
```

### QuotaBackend

```rust
pub trait QuotaBackend: Send + Sync + 'static {
    type Hold: QuotaHold;
    fn try_reserve(&self, identity_id: i64, estimated_cost: u64)
        -> impl Future<Output = Result<Self::Hold, QuotaExhausted>> + Send;
    fn balance(&self, identity_id: i64)
        -> impl Future<Output = Result<QuotaBalance, QuotaError>> + Send;
}

pub trait QuotaHold: Send + 'static {
    fn settle(self, actual_cost: u64)
        -> impl Future<Output = Result<(), QuotaError>> + Send;
    // Drop impl: 如未 settle，不退回预扣（保守扣费）
}
```

### AffinityBackend

```rust
pub trait AffinityBackend: Send + Sync + 'static {
    fn get_binding(&self, key: &str) -> impl Future<Output = Option<String>> + Send;
    fn set_binding(&self, key: &str, credential_id: &str, ttl: Duration)
        -> impl Future<Output = Result<(), anyhow::Error>> + Send;
    fn remove_binding(&self, key: &str)
        -> impl Future<Output = Result<(), anyhow::Error>> + Send;
}
```

**设计原则：**
- 用 RPITIT（`impl Future`）而非 `async_trait`，内存实现编译后零 heap allocation
- 内存实现中 async 方法返回 `std::future::ready()`
- SDK 提供 `InMemoryRateLimit`、`InMemoryQuota`、`InMemoryAffinity` 默认实现
- App 层可提供 Redis 实现（通过 gproxy-core 的 feature flag）

---

## 三、SDK ProviderStore 重构

### 从 ArcSwap<HashMap> 到 DashMap 增量更新

```rust
pub struct ProviderStore {
    providers: DashMap<String, Arc<ProviderRuntime>>,
    credential_index: DashMap<String, (String, usize)>,  // cred_id → (provider, index)
    event_tx: broadcast::Sender<EngineEvent>,
}
```

### 三个接口 trait

| Trait | 用途 | 调用方 |
|-------|------|--------|
| `ProviderRegistry` (只读查询) | `get_provider`, `list_providers`, `get_credential`, `list_credentials` | App 层 admin query、permission 校验 |
| `ProviderMutator` (增量写入) | `upsert_provider`, `remove_provider`, `upsert_credential`, `remove_credential`, `replace_all` | App 层 admin handler、bootstrap |
| `EngineEventSource` (变更通知) | `subscribe() → broadcast::Receiver<EngineEvent>` | App 层 HealthSyncer worker |

### GproxyEngine 保留为 Facade

```rust
pub struct GproxyEngine<A: AffinityBackend, R: RateLimitBackend> {
    store: Arc<ProviderStore>,
    affinity: A,
    rate_limit: R,
    client: wreq::Client,
    spoof_client: Option<wreq::Client>,
}
```

App 层 bootstrap 时用 **enum dispatch** 注入 backend（兼顾零开销和运行时灵活性）。

### 执行路径不动

`execute_request → retry_with_credentials → channel.prepare_request` 核心热路径保持不变。

---

## 四、Quota 预扣除模型

### 流程

```
请求进入 → 估算 cost（input_tokens × price + output_cap × price）
         → quota.try_reserve(user_id, estimated_cost) 返回 Hold guard
         → engine.execute_request()
         → 流结束，从 usage 字段提取 actual_cost
         → hold.settle(actual_cost) 退回差额
         → 如果 panic/崩溃，Hold::drop() 不退回（保守扣费）
```

### output_cap 预估策略

1. 请求中的 `max_tokens` / `max_completion_tokens`（Claude 必填 / OpenAI 可选）
2. 保守默认值 2048 tokens（不用模型最大值，避免假性拒绝）

### Hold 实现

InMemory 版用 `Arc<AtomicU64>` + Drop，不需要 async drop。Redis 版用 Redis DECRBY + TTL 自动退还。

---

## 五、写路径分离

| 类型 | 路径 | 模式 |
|------|------|------|
| Admin CRUD (provider/user/model/...) | Handler → Repository (sync .await) → notify domain service | Control plane，同步，强一致 |
| Usage/Request log | Handler → `mpsc::Sender` → UsageSink → batch insert | Data plane，异步，最终一致 |
| Quota delta | QuotaBackend 内存递增 → QuotaReconciler 定期对账 | 异步，30s 周期 |
| Credential health | SDK broadcast → HealthBroadcaster → debounced persist | 异步，500ms debounce |

### Storage 层改造

`StorageWriteEvent` 26 变体 enum → 拆为：
- **Repository trait per aggregate root**：`ProviderRepository`, `UserRepository`, `ModelRepository` 等
- **WriteSink trait**：`fn send_usage(&self, record)`, `fn send_request_log(&self, record)` — fire-and-forget

---

## 六、Domain Services（`gproxy-core`）

| Service | 管辖数据 | 存储模式 |
|---------|---------|---------|
| `IdentityService` | users + keys | ArcSwap（bootstrap 加载，admin CRUD 原子替换） |
| `PolicyService` | permissions + file_permissions + rate_limits | ArcSwap + RateLimitBackend |
| `QuotaService` | quota 余额 + 预扣 | QuotaBackend（内存或 Redis） |
| `AliasService` | model aliases | ArcSwap<HashMap<String, String>> |
| `FileService` | user_files + claude_files | ArcSwap |
| `RoutingService` | models + model→provider 映射 | ArcSwap + SDK ProviderRegistry 查询 |

Bootstrap: 每个 service 提供 `replace_all(data)` 用于全量加载。
Runtime: 每个 service 提供 `upsert/remove` 用于单条 admin 操作。
Reload: 两阶段（先加载到临时变量，再连续 replace_all），接受微秒级跨 service 不一致窗口。

---

## 七、Background Workers

| Worker | 触发 | Batch 策略 | Shutdown |
|--------|------|-----------|---------|
| **UsageSink** | mpsc channel | 100 条 或 500ms | drain channel → final flush (5s timeout) |
| **QuotaReconciler** | 30s tick | N/A | 直接退出（无状态丢失风险） |
| **HealthBroadcaster** | SDK watch channel | 500ms debounce, 去重 | drain → final flush |
| **RateLimitGC** | 60s tick | 扫描清理过期窗口 | 直接退出 |

统一用 `CancellationToken` + `JoinHandle` 管理生命周期。

---

## 八、多实例未来路径

当前设计为多实例预留了 trait 抽象：

| 状态 | 单实例 | 多实例 |
|------|--------|--------|
| 限流计数 | `InMemoryRateLimit` | `RedisRateLimit`（INCR + TTL） |
| Quota | `InMemoryQuota` | `RedisQuota`（DECRBY + TTL hold） |
| Affinity | `InMemoryAffinity` | `RedisAffinity`（SET + TTL） |
| Credential 健康 | 本地 Mutex（不跨实例） | 可接受，各实例独立学习 |
| 配置同步 | 不需要 | 各实例 `/admin/reload` 或 Redis pub/sub 广播 |

切换只需在 `apps/gproxy/src/main.rs` bootstrap 时选择不同的 backend 实现，不改任何 SDK 或 service 代码。

---

## 九、Middleware 重构

| Middleware | 依赖 | 变化 |
|-----------|------|------|
| `classify` | SDK `gproxy-routing::classify_route()` | thin axum wrapper |
| `request_model` | SDK `gproxy-routing::extract_model()` | thin axum wrapper |
| `provider_prefix` | SDK `gproxy-routing::split_provider_prefix()` | thin axum wrapper |
| `model_alias` | App `AliasService` | 保留在 App |
| `permission` | App `PolicyService` | 保留在 App |
| `rate_limit` | App `PolicyService` + `RateLimitBackend` | 保留在 App |
| `sanitize` | 无依赖 | 保留在 App（gproxy 安全策略） |

---

## 十、实施阶段

### Phase 1: SDK 重组（不改 App）
1. 创建 `gproxy-routing` crate，从 middleware 提取 classify/model_extraction/provider_prefix 纯逻辑
2. 在 `gproxy-provider` 中定义 `RateLimitBackend`/`QuotaBackend`/`AffinityBackend` trait + 内存实现
3. ProviderStore 改用 DashMap，暴露 `ProviderRegistry`/`ProviderMutator`/`EngineEventSource` trait
4. GproxyEngine 泛型化，构建时注入 backend
5. `gproxy-sdk` 更新 re-export
6. 验证：`cd sdk && cargo build` 独立编译通过

### Phase 2: App 层重建
1. 创建 `gproxy-core`，实现 6 个 domain service
2. 重写 `gproxy-storage`：Repository trait per aggregate root + WriteSink
3. 重写 `gproxy-api`：middleware 改为 thin wrapper，handler 调用 domain service
4. 重写 `apps/gproxy`：bootstrap 装配所有 service + worker，注入 backend
5. 实现 Quota 预扣除模型
6. 实现 4 个 background worker
7. 验证：全 workspace `cargo build` 通过

### Phase 3: 集成验证
1. 启动 app，通过 admin API 创建 provider/credential/user
2. 代理请求验证：scoped + unscoped + file + WebSocket
3. Quota 预扣除验证：余额不足拒绝、正常扣费、崩溃不退回
4. 限流验证：RPM/RPD 触发 429
5. 流式响应 usage tracking
6. `POST /admin/reload` 验证全量重建
7. Graceful shutdown 验证日志不丢失

---

## 关键文件清单

### SDK 层（修改/新建）
- `sdk/gproxy-routing/` — **新 crate**
- `sdk/gproxy-provider/src/store.rs` — DashMap 重构 + 三个 trait
- `sdk/gproxy-provider/src/engine.rs` — 泛型化
- `sdk/gproxy-provider/src/backends/` — **新模块**：trait 定义 + InMemory 实现
- `sdk/gproxy-provider/src/retry.rs` — 集成 AffinityBackend
- `sdk/gproxy-sdk/src/lib.rs` — 更新 re-export

### App 层（新建/重写）
- `crates/gproxy-core/` — **新 crate**：6 个 domain service
- `crates/gproxy-storage/src/` — Repository pattern 重写
- `crates/gproxy-api/src/` — handler + middleware 重写
- `apps/gproxy/src/main.rs` — bootstrap + worker 重写
- `apps/gproxy/src/workers.rs` — **新文件**：4 个 background worker

---

## 十一、Codex 多 Agent 执行编排

### 执行原则

- **Claude 计划 → Codex 实施 → Claude+Codex 对审收敛 → 修改 → 下一步** 循环
- 顺序执行（不用 git worktree），Codex agent 在 background 运行
- 用户不需要干预

### Review 对审流程（每个 Checkpoint）

每个 CP 的 review 不是 Claude 单方面判定，而是 **Claude 和 Codex 对审到收敛**：

```
Step 1: Claude 先做初审
  - 阅读变更代码，按检查清单逐项审查
  - 列出发现的问题（如有）
  - 形成初步审查意见

Step 2: 派 Codex code-reviewer agent 做独立 review
  - Codex 独立阅读代码，给出自己的审查意见
  - 可以同意 Claude 的判断，也可以提出异议

Step 3: 对审收敛
  - 如果 Claude 和 Codex 意见一致 → 通过（或按一致意见修改后通过）
  - 如果有分歧 → Claude 和 Codex 讨论，Claude 判断更高优先级
  - 讨论收敛后，对需要修改的点派 Codex 修改
  - 修改后再走一轮 review 直到双方都无异议

Step 4: 通过条件
  - Claude review 无异议 AND Codex review 无异议 → 进入下一步
  - 如果反复未收敛（>3轮），Claude 最终决策
```

**Claude 判断优先级高于 Codex**，但 Codex 的异议必须被认真考虑和回应，不能直接忽略。

### 任务依赖图

```
Phase 1 (SDK):
  1a (routing crate) ──┐
  1b (backend traits) ──┼── 1c (store DashMap) ── 1d (engine 泛型化) ── 1e (sdk re-export)

Phase 2 (App):
  2a (core services) ── 2b (storage repo) ── 2c (api handlers) ── 2d (bootstrap + workers)

Phase 3:
  3a (集成测试)
```

1a 和 1b 文件集不相交，可并行。其余严格串行。

### 执行编排表

| 批次 | Agent | 任务 | 预计复杂度 | 验证 |
|------|-------|------|-----------|------|
| Batch 1 (并行) | A | 1a: 创建 gproxy-routing crate | 中 (~950行迁移) | `cargo build -p gproxy-routing` |
| Batch 1 (并行) | B | 1b: Backend trait + InMemory 实现 | 高 (~新增500行) | `cargo build -p gproxy-provider` |
| **CP-1** | Claude 初审 → Codex review → 对审收敛 | trait 签名 + routing API + Cargo.toml | — | — |
| Batch 2 | C | 1c: ProviderStore DashMap 重构 | 高 (~1066行重构) | `cargo build -p gproxy-provider` |
| **CP-2** | Claude 初审 → Codex review → 对审收敛 | DashMap 安全性 + 向后兼容 + retry 语义 | — | — |
| Batch 3 | D | 1d: GproxyEngine 泛型化 | 高 (~1393行) | `cargo build -p gproxy-provider` |
| **CP-3** | Claude 初审 → Codex review → 对审收敛 | 泛型约束最小化 + 下游编译 | — | — |
| Batch 4 | E | 1e: gproxy-sdk re-export 更新 | 低 | `cd sdk && cargo build` |
| **CP-4** | Claude 初审 → Codex review → 对审收敛 | **Phase 1 完整 review** | — | `cargo build --workspace` |
| Batch 5 | F | 2a: gproxy-core domain services | 高 (~830行拆分) | `cargo build -p gproxy-core` |
| **CP-5** | Claude 初审 → Codex review → 对审收敛 | service 边界 + DTO 设计 | — | — |
| Batch 6 | G | 2b: gproxy-storage Repository | 高 | `cargo build -p gproxy-storage` |
| **CP-6** | Claude 初审 → Codex review → 对审收敛 | Repository trait 完整性 | — | — |
| Batch 7 | H | 2c: gproxy-api thin handlers | 高 (~975行重写) | `cargo build -p gproxy-api` |
| Batch 8 | I | 2d: apps/gproxy bootstrap + workers | 中 | `cargo build` |
| **CP-7** | Claude 初审 → Codex review → 对审收敛 | **Phase 2 完整 review** | — | `cargo build && cargo clippy` |
| Batch 9 | J | 3a: 端到端测试 | 中 | `cargo test --workspace` |
| **CP-8** | Claude 初审 → Codex review → 对审收敛 | **最终验收** | — | — |

### 每个 Codex Agent Prompt 结构

每个 prompt 包含四个部分：

```xml
<task>
精确的步骤、文件路径、要创建/修改的内容
</task>

<verification_loop>
每步完成后的编译/测试命令
</verification_loop>

<action_safety>
不允许修改的文件列表（防止越界）
</action_safety>

<rust_conventions>
命名、错误处理、文档注释、并发原语等规范
</rust_conventions>
```

### Review Checkpoint 详细流程

**Step 1: 自动验证**
```bash
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

**Step 2: Claude 初审检查清单**
```
□ 所有 pub item 有 /// 文档注释
□ 错误类型统一用 thiserror（不用 anyhow）
□ 无 unwrap() / expect()（测试代码除外）
□ trait 标记 Send + Sync + 'static
□ 无循环依赖
□ 无 dead code
□ RPITIT 用法正确（Rust 2024 edition）
□ 架构设计与计划一致
□ 代码可读性、命名合理性
```

**Step 3: 派 Codex code-reviewer agent**

使用 `superpowers:code-reviewer` agent，prompt 包含：
- 本次变更的文件列表和 diff
- 计划文档中对应章节作为 review 标准
- 要求 Codex 关注：正确性、并发安全、API 设计、遗漏的边界条件

**Step 4: 对审收敛**

Claude 汇总双方意见：
- 一致同意 → 通过
- 有分歧 → Claude 和 Codex 讨论（Claude 优先级高）
- 需要修改 → 派 Codex 实施修改 → 重新 review
- 最多 3 轮，超过则 Claude 最终裁定

### 质量保证规范

- **错误处理**：统一 thiserror（SDK + App）
- **命名**：crate kebab-case，module snake_case，type CamelCase
- **可见性**：默认 `pub(crate)`，对外接口显式 `pub`
- **并发**：DashMap + ArcSwap，避免粗粒度 Mutex
- **trait 设计**：Rust 2024 RPITIT，不用 async_trait 宏
- **文档**：所有 pub item 必须 `///` 注释
- **格式化**：严格 `cargo fmt`

### 预估时间线

```
Day 1:  Batch 1 (1a+1b 并行) → CP-1 → Batch 2 (1c) → CP-2
Day 2:  Batch 3 (1d) → CP-3 → Batch 4 (1e) → CP-4 (Phase 1 完整 review)
Day 3:  Batch 5 (2a) → CP-5 → Batch 6 (2b) → CP-6
Day 4:  Batch 7 (2c) → Batch 8 (2d) → CP-7 (Phase 2 完整 review)
Day 5:  Batch 9 (3a 测试) → CP-8 最终验收
```

### 风险应对

| 风险 | 概率 | 应对 |
|------|------|------|
| Cargo.toml 合并冲突（1a+1b 并行） | 高 | Claude 手动合并 workspace members |
| store.rs 重构引入 bug | 中 | CP-2 重点 review DashMap 操作 + retry 语义 |
| trait 签名不适合下游 | 中 | CP-1 严格 review，Phase 2 前可调整 |
| Rust 2024 RPITIT 兼容性 | 低 | 回退到 async_trait |
| 功能回归 | 中 | Phase 3 端到端测试覆盖核心路径 |

---

## 十二、验证方案

### Phase 1 验证
```bash
# SDK 独立编译
cd sdk && cargo build
# 全 workspace 编译
cargo build --workspace
# Lint
cargo clippy --workspace -- -D warnings
```

### Phase 2 验证
```bash
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

### Phase 3 验证
```bash
# 单元测试
cargo test --workspace
# 启动验证（手动）
cargo run --bin gproxy -- --admin-key test123
# admin API 验证
curl -X POST http://localhost:8080/admin/health -H "Authorization: Bearer test123"
```
