---
title: Rewrite Rules
description: 使用 v2 rule set 在协议 transform 之后、发往上游之前改写 provider-native JSON body 和 header。
---

v2 的 rewrite 规则放在可复用的 **rule set** 中。一个 rule set 可以通过 `provider_rule_sets` 绑定到多个 provider；绑定后的规则会在协议 transform 之后、channel prepare 之前执行。

```text
client request
  -> classify / auth / route / balance
  -> protocol transform to provider-native body
  -> process rule sets
  -> channel shape_request
  -> channel prepare / upstream send
```

本页描述当前已实现的 rule kind。`transform` 是通用的 `locate + actions (+ limit)` 引擎；provider-specific 行为尽量放在配置或 console preset 中。

## Rule Set 模型

| 记录 | 用途 |
| --- | --- |
| `rule_sets` | 命名的可复用集合。 |
| `rules` | 集合中的具体规则。 |
| `provider_rule_sets` | 把 rule set 按 `sort_order` 绑定到 provider。 |

Snapshot 重建时会编译启用的 rule set。无法解析的规则会 warn 并跳过。Provider attachment 会按绑定顺序展开，然后按固定 kind 顺序排序。

## 通用字段

每条 rule 包含：

- `kind`：`system_text`、`cache_breakpoint`、`rewrite`、`transform`、`header` 之一。
- `config_json`：kind-specific 配置。
- `filter_model_pattern`：可选 glob，匹配去掉前缀后的上游 model 名称。
- `filter_operation_keys`：可选 Operation 列表，例如 `generate_content` 或 `stream_generate_content`。
- `sort_order` 和 `enabled`。

过滤条件按 AND 组合。省略的维度表示匹配全部。

## `rewrite`

`rewrite` 修改 JSON body path：

```json
{
  "path": "stream_options.include_usage",
  "action": "set",
  "value_json": true
}
```

支持的 action：

| Action | 行为 |
| --- | --- |
| `set` | 创建缺失的 object parent，并在 leaf 写入 `value_json`。 |
| `delete` | 删除存在的 object key 或 array element；缺失路径跳过。 |
| `merge` | 把 object 类型的 `value_json` shallow-merge 到路径上的现有 object。 |

Path 用点分隔。支持 object key 和数字 array index，例如 `messages.0.content`。这是刻意保持简单、fail-soft 的路径模型。

## `transform`

`transform` 对匹配的 JSON path 或序列化后的 body 做通用文本替换：

```json
{
  "phase": "request",
  "locate": { "match": "\\binternal-tool\\b" },
  "actions": [{ "op": "replace_text", "with": "tool" }]
}
```

结构化值可使用 `locate.path`，路径支持点分隔、`*` 通配符和可选的精确 `from` guard：

```json
{
  "phase": "response",
  "locate": { "path": "content.*.name" },
  "actions": [{ "op": "replace_text", "from": "tasklist", "with": "todowrite" }]
}
```

`phase` 可为 `request`、`response` 或 `both`，默认是 `request`。Regex 是序列化后的宽泛匹配，因此 pattern 要尽量精确。

## `header`

`header` 设置或合并请求 header：

```json
{
  "name": "anthropic-beta",
  "value": "extended-cache-ttl-2025-04-11",
  "mode": "merge"
}
```

`override` 会替换 header。`merge` 会用逗号追加并去重，适合 `anthropic-beta` 这类 list-valued header。

## 固定执行顺序

规则按固定顺序执行，不完全按绑定顺序：

```text
system_text -> cache_breakpoint -> rewrite -> transform -> header
```

同一 kind 内保留 set 和 rule 的 sort order。错误或不适用的规则不会打断流量，只会 warn 并跳过。
