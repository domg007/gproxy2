---
title: 凭证选择与缓存亲和池
description: 凭证选择模式、内部缓存亲和池设计、命中判定，以及 OpenAI/Claude/Gemini 的缓存命中实践。
---

## 为什么需要这一页

一个 provider 配置多凭证时，缓存命中率与凭证选择策略强耦合：

- 缓存敏感请求频繁换凭证，通常会降低上游缓存命中率。
- 所有流量固定单凭证，会降低吞吐与故障切换能力。

GPROXY 用“凭证选择模式 + 进程内缓存亲和池”来平衡这两点。

## 凭证选择模式

在 `channels.settings` 中配置：

- `credential_round_robin_enabled`（默认 `true`）
- `credential_cache_affinity_enabled`（默认 `true`）

最终模式：

| `credential_round_robin_enabled` | `credential_cache_affinity_enabled` | 最终模式 | 行为 |
|---|---|---|---|
| `false` | `false/true` | `StickyNoCache` | 不轮询，不用亲和池，始终选当前可用凭证里 id 最小者 |
| `true` | `true` | `RoundRobinWithCache` | 在可用凭证中轮询并启用亲和匹配 |
| `true` | `false` | `RoundRobinNoCache` | 在可用凭证中轮询，不做亲和匹配 |

说明：

- `StickyWithCache` 已移除，不再支持。
- 关闭轮询时会强制关闭亲和池。
- 历史字段 `credential_pick_mode`、`cache_affinity_enabled` 仍会被解析。

## 内部缓存亲和池设计（v1）

GPROXY 维护进程内 map：

- key：`"{channel}::{affinity_key}"`
- value：`{ credential_id, expires_at }`
- 存储：`DashMap<String, CacheAffinityRecord>`

这仍是 v1 设计：不引入 v2 key 前缀，不改存储结构。

## 命中判定与重试行为

仅 `RoundRobinWithCache` 使用多候选 hint：

- `CacheAffinityHint { candidates, bind }`
- 每个候选为 `{ key, ttl_ms }`

处理流程：

1. 按协议分块规则构建候选键（有优先级顺序）。
2. 按顺序命中第一个可用且未过期的映射凭证。
3. 若未命中，退回普通轮询。
4. 请求成功后，总是写入 `bind` 键。
5. 若本次由某候选键命中，还会刷新该命中键 TTL。
6. 若本次亲和命中失败并重试，只清理本次命中的那个键。

## 协议键推导与 TTL 规则

四类内容生成请求不再使用整包 body hash，而是按“可缓存前缀分块 + 滚动哈希”。

统一规则：

- 块级 canonical JSON：对象 key 排序、去除 `null`、数组保序。
- 滚动哈希：`prefix_i = sha256(seed + block_1 + ... + block_i)`。
- 非 Claude 采样：
  - 边界 `<=64` 全量
  - 边界 `>64` 取“前8 + 后56”
  - 优先级按最长前缀优先
- `stream` 不参与键计算。

### OpenAI Chat Completions

分块顺序：

- `tools[]`
- `response_format.json_schema`
- `messages[]`（按 content block 细分）

键格式：

- `openai.chat:ret={ret}:k={prompt_cache_key_hash|none}:h={prefix_hash}`

TTL：

- `prompt_cache_retention == "24h"` -> 24h
- 其他 -> 5m

### OpenAI Responses

分块顺序：

- `tools[]`
- `prompt(id/version/variables)`
- `instructions`
- `input`（按 item/content block 细分）

键格式：

- `openai.responses:ret={ret}:k={prompt_cache_key_hash|none}:h={prefix_hash}`

TTL：

- `prompt_cache_retention == "24h"` -> 24h
- 其他 -> 5m

默认不参与前缀键：

- `reasoning`
- `max_output_tokens`
- `stream`

### Claude Messages

分块层级：

- `tools[] -> system -> messages.content[]`

断点来源：

- 显式断点：块上有 `cache_control`
- 顶层自动断点：请求有顶层 `cache_control` 时，取最后可缓存块（必要时向前回退）

候选构建：

- 每个断点最多回看 20 个边界
- 合并去重
- 优先级：更晚断点优先；同断点内更长前缀优先

键格式：

- `claude.messages:ttl={5m|1h}:bp={explicit|auto}:h={prefix_hash}`

TTL：

- 断点 `ttl == "1h"` -> 1h
- 顶层自动 `cache_control: {"type":"ephemeral"}`（无 ttl）-> 1h
- 否则 -> 5m

若既无显式断点也无顶层 `cache_control`，则不生成 Claude 亲和 hint。

### Gemini GenerateContent / StreamGenerateContent

若存在 `cachedContent`：

- 键：`gemini.cachedContent:{sha256(cachedContent)}`
- TTL：60m

否则走前缀模式：

- 分块顺序：`systemInstruction -> tools[] -> toolConfig -> contents[].parts[]`
- 键：`gemini.generateContent:prefix:{prefix_hash}`
- TTL：5m

默认不纳入键：

- `generationConfig`
- `safetySettings`

## Claude 与 ClaudeCode 顶层缓存注入

当开启 `enable_top_level_cache_control` 且请求本身没有顶层 `cache_control` 时，GPROXY 会注入：

```json
{"type":"ephemeral"}
```

该行为适用于 Claude 与 ClaudeCode 的消息生成请求。自动模式下的实际 TTL 由 Anthropic 服务端决定。

## 上游缓存机制（与 GPROXY 内部实现解耦）

### OpenAI

- 以前缀缓存为主。
- 维持稳定模型、稳定 system/tools/prefix 有助于命中。
- `prompt_cache_key` 与 retention 会影响缓存行为。

### Claude

- 支持显式 block 断点与顶层自动缓存。
- 对前缀、断点位置、块顺序敏感。

### Gemini

- 主要依赖 `cachedContent` 资源复用。
- 复用同一 `cachedContent` 句柄通常更容易命中。
- 当前 GPROXY 仅覆盖内容生成，不提供 `cachedContent` 管理路由。

## 提高命中率建议

1. 保持前缀稳定：model、tools、system、长上下文顺序尽量不变。
2. 缓存敏感业务用 `RoundRobinWithCache`。
3. 短缓存窗口内避免凭证频繁抖动。
4. Prompt 差异很大的流量拆分到不同 provider/channel。
5. 仅在需要自动缓存时开启顶层 cache control。
6. Gemini 场景尽量复用 `cachedContent`。

## 配置示例

轮询 + 亲和池：

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

不轮询（固定最小 id 可用凭证）：

```toml
[channels.settings]
credential_round_robin_enabled = false
```
