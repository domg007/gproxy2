---
title: 凭证选择与缓存亲和池
description: 凭证选择模式、内部缓存亲和池设计、命中判定，以及 OpenAI/Claude/Gemini 的缓存命中实践。
---

## 为什么需要这一页

当一个 provider 配置了多凭证时，缓存命中率和凭证选择策略是强耦合的：

- 缓存相关请求频繁切换凭证，通常会降低上游缓存命中率。
- 所有请求都硬绑定单凭证，会降低吞吐和容错能力。

GPROXY 通过“凭证选择模式 + 内部缓存亲和池”来平衡这两个目标。

## 凭证选择模式配置

在 `channels.settings` 配置：

- `credential_round_robin_enabled`（默认 `true`）
- `credential_cache_affinity_enabled`（默认 `true`）

最终模式如下：

| `credential_round_robin_enabled` | `credential_cache_affinity_enabled` | 最终模式 | 行为 |
|---|---|---|---|
| `false` | `false/true` | `StickyNoCache` | 不轮询，不用亲和池，始终选择当前可用凭证中 `id` 最小者 |
| `true` | `true` | `RoundRobinWithCache` | 在可用凭证中随机选择，并启用缓存亲和池 |
| `true` | `false` | `RoundRobinNoCache` | 在可用凭证中随机选择，不启用亲和池 |

说明：

- `StickyWithCache` 已被明确移除，不再支持。
- 只要关闭轮询，就会强制关闭缓存亲和池。
- 仍兼容旧字段：`credential_pick_mode` 与 `cache_affinity_enabled`。

## 内部缓存亲和池设计

GPROXY 在内存中维护一个 affinity map：

- key：`"{channel}::{affinity_key}"`
- value：`{ credential_id, expires_at }`
- 范围：进程内（不持久化，不跨实例共享）

仅在 `RoundRobinWithCache` 生效，处理流程：

1. 从请求体/协议提取 `CacheAffinityHint`。
2. 用 scoped key 查询 affinity map。
3. 若记录存在、未过期，且目标凭证当前可用，则优先命中该凭证。
4. 否则在当前可用凭证中随机选择。
5. 请求成功后，写入/刷新 affinity 记录。
6. 若本次是“亲和命中”但请求失败并触发重试，会先清理该 key，再继续尝试其他可用凭证。

凭证的健康状态与冷却仍然优先，亲和池不会强行使用不可用凭证。

## 内部缓存亲和池命中判定机制

需要同时满足两个条件：

1. 当前请求能够推导出稳定 `affinity_key`。
2. affinity map 中有未过期记录，且记录对应凭证仍在可用列表内。

任一条件不满足，就退回普通的随机凭证选择。

## GPROXY 中三类协议的亲和 key 与 TTL 计算

当前逻辑在 `retry.rs`：

### OpenAI 风格（`/v1/responses`、`/v1/chat/completions`）

- key 规则：
  - 若有非空 `prompt_cache_key`，优先用它
  - 否则对整个请求 JSON body 做 SHA-256
- TTL 规则：
  - `prompt_cache_retention == "24h"` 时用 `24h`
  - 否则用 `5m`

### Claude 风格（`/v1/messages`）

- key 规则：对请求 JSON body 做 SHA-256
- TTL 规则：
  - 顶层 `cache_control.ttl == "1h"` 时用 `1h`
  - 否则用 `5m`

### Gemini 风格（`:generateContent`、`:streamGenerateContent`）

- key 规则：
  - 若有非空 `cachedContent`，优先用它
  - 否则对 `{ model, body }` 做 SHA-256
- TTL 规则：
  - 当前 GPROXY 亲和池固定 `5m`

## 上游缓存机制说明（与 GPROXY 实现解耦）

### OpenAI

- 主要是前缀缓存思路。
- 不同接口对缓存控制字段支持程度不同。
- 稳定前缀、稳定模型、稳定 tools/system 通常能提升命中率。

### Claude

- 支持显式 block 级缓存断点。
- 也支持 Messages API 顶层自动缓存控制。
- 常见 TTL 为 `5m`，部分场景支持 `1h`。

### Gemini

- 核心是 Context Caching / `cachedContent` 资源复用。
- 复用同一 cached content 句柄通常更容易命中缓存。
- 当前 GPROXY 仅覆盖 Gemini 内容生成链路，没有专门封装创建缓存资源的辅助接口。

## 提高缓存命中率的实用建议

1. 保持前缀内容稳定：system、tools、长上下文顺序、model 名称尽量不变。
2. 缓存敏感流量优先用 `RoundRobinWithCache`。
3. 在缓存窗口内避免不必要的凭证抖动。
4. Prompt 差异很大的业务拆到不同 provider/channel。
5. Claude/ClaudeCode 仅在需要自动缓存时开启 `enable_top_level_cache_control`。
6. Gemini 若上游工作流支持，优先复用 `cachedContent`。

## 使用方式示例

轮询 + 缓存亲和池：

```toml
[channels.settings]
credential_round_robin_enabled = true
credential_cache_affinity_enabled = true
```

轮询 + 不使用亲和池：

```toml
[channels.settings]
credential_round_robin_enabled = true
credential_cache_affinity_enabled = false
```

不轮询（固定最小 id 可用凭证，不使用亲和池）：

```toml
[channels.settings]
credential_round_robin_enabled = false
```
