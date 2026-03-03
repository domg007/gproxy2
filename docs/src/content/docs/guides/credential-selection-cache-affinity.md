---
title: Credential Selection and Cache Affinity
description: Pick modes, internal cache-affinity pool design, hit judgment, and OpenAI/Claude/Gemini cache-hit practices.
---

## Why this page exists

When a provider has multiple credentials, cache hit rate and credential selection are coupled:

- If cache-sensitive requests keep switching credentials, upstream cache hit rate usually drops.
- If everything is pinned to one credential, throughput and failover degrade.

GPROXY balances this with pick modes plus an internal in-memory cache-affinity pool.

## Pick mode configuration

Configure these fields in `channels.settings`:

- `credential_round_robin_enabled` (default `true`)
- `credential_cache_affinity_enabled` (default `true`)

Effective modes:

| `credential_round_robin_enabled` | `credential_cache_affinity_enabled` | Effective mode | Behavior |
|---|---|---|---|
| `false` | `false/true` | `StickyNoCache` | no round-robin, no affinity pool, always pick the smallest available credential id |
| `true` | `true` | `RoundRobinWithCache` | round-robin among eligible credentials with affinity matching |
| `true` | `false` | `RoundRobinNoCache` | round-robin among eligible credentials, no affinity matching |

Notes:

- `StickyWithCache` is intentionally not supported.
- If round-robin is disabled, affinity is forced off.
- Legacy field `credential_pick_mode` is still parsed for compatibility.

## Internal affinity pool design (v1)

GPROXY keeps a process-local map:

- key: `"{channel}::{affinity_key}"`
- value: `{ credential_id, expires_at }`
- store: `DashMap<String, CacheAffinityRecord>`

This is still the v1 pool format (no v2 namespace, no storage schema change).

## Hit judgment and retry behavior

`RoundRobinWithCache` uses a multi-candidate hint:

- `CacheAffinityHint { candidates, bind }`
- each candidate has `{ key, ttl_ms, key_len }`

Selection flow:

1. Build candidate keys from request body with protocol-specific block/prefix rules.
2. Scan candidates in order. The scan stops at the first miss, or at the first hit whose mapped credential is not currently eligible.
3. For the contiguous hit prefix, sum `key_len` by credential and pick the credential with the largest total score.
4. If scores tie, pick the credential that appears earlier in the current eligible list.
5. If no candidate is usable, fall back to normal round-robin among eligible credentials.
6. On success, always bind the `bind` key and refresh the matched key (if any).
7. If an affinity-picked attempt fails and retries, only clear the matched key for that attempt.

Important:

- This is GPROXY internal routing affinity, not the upstream provider's native cache-hit decision.

## Key derivation and TTL rules by protocol

GPROXY no longer uses whole-body hash for these content-generation requests. It uses canonicalized block prefixes.

Shared rules:

- Canonical JSON per block: sorted object keys, `null` removed, arrays keep order.
- Rolling prefix hash: `prefix_i = sha256(seed + block_1 + ... + block_i)`.
- Non-Claude candidate sampling:
  - `<=64` boundaries: all
  - `>64`: first 8 and last 56
  - match priority: longest prefix first
- `stream` does not participate in key derivation.

### OpenAI Chat Completions

Block order:

- `tools[]`
- `response_format.json_schema`
- `messages[]` (split by content blocks)

Key format:

- `openai.chat:ret={ret}:k={prompt_cache_key_hash|none}:h={prefix_hash}`

TTL:

- `prompt_cache_retention == "24h"` -> 24h
- otherwise -> 5m

### OpenAI Responses

Block order:

- `tools[]`
- `prompt(id/version/variables)`
- `instructions`
- `input` (split item/content blocks)

Key format:

- `openai.responses:ret={ret}:k={prompt_cache_key_hash|none}:h={prefix_hash}`

TTL:

- `prompt_cache_retention == "24h"` -> 24h
- otherwise -> 5m

Not included in prefix key:

- `reasoning`
- `max_output_tokens`
- `stream`

### Claude Messages

Block hierarchy:

- `tools[] -> system -> messages.content[]`

Breakpoints:

- explicit: block has `cache_control`
- automatic: top-level `cache_control` exists, then use the last cacheable block (fallback backward if needed)

Candidates:

- for each breakpoint, include up to 20 lookback boundaries
- merge and dedupe candidates
- priority: later breakpoint first, then longer prefix first

Key format:

- `claude.messages:ttl={5m|1h}:bp={explicit|auto}:h={prefix_hash}`

TTL:

- breakpoint `ttl == "1h"` -> 1h
- auto top-level `cache_control: {"type":"ephemeral"}` (no ttl) -> 1h
- otherwise -> 5m

If request has no explicit breakpoint and no top-level `cache_control`, affinity hint is not generated.

### Gemini GenerateContent / StreamGenerateContent

If `cachedContent` exists:

- key: `gemini.cachedContent:{sha256(cachedContent)}`
- TTL: 60m

Otherwise prefix mode:

- block order: `systemInstruction -> tools[] -> toolConfig -> contents[].parts[]`
- key: `gemini.generateContent:prefix:{prefix_hash}`
- TTL: 5m

Not included by default:

- `generationConfig`
- `safetySettings`

## Claude and ClaudeCode top-level cache control

When `enable_top_level_cache_control` is enabled and request has no top-level `cache_control`, GPROXY injects:

```json
{"type":"ephemeral"}
```

This applies to Claude and ClaudeCode message generation requests. Anthropic side decides the effective TTL for this automatic mode.

## Upstream cache mechanisms (provider-side)

These are provider behaviors, independent from GPROXY affinity internals.

### OpenAI

- Prompt caching is exact-prefix based.
- Requests are routed by a prefix hash; `prompt_cache_key` is combined with that routing key.
- Keep static content (instructions/tools/examples/images schema) in the prefix, and move variable content to the tail.
- Retention (`in_memory` vs `24h`) affects lifetime behavior, but cache matching remains prefix-oriented.

### Claude

- Prefix hierarchy is `tools -> system -> messages`.
- Supports explicit block-level breakpoints and automatic top-level `cache_control`.
- Uses backward sequential checking with a 20-block lookback window around breakpoints.
- Cacheability depends on block eligibility; ordering and breakpoint placement directly impact hit rate.

### Gemini

- Explicit context caching is centered around `cachedContent` reuse (cached content is treated as prompt prefix).
- Implicit caching is provider-managed and works best when similar prefixes are sent close in time.
- Reusing the same `cachedContent` handle typically improves explicit-cache hit behavior.
- GPROXY currently supports generation routes and does not expose cached-content management routes.

## Practical tips

1. Keep prefix content byte-stable (model, tools, system, long context ordering).
2. Use `RoundRobinWithCache` for cache-sensitive traffic.
3. Avoid unnecessary credential churn inside short cache windows.
4. Split very different prompt workloads into different channels/providers.
5. Enable top-level cache control only when you want automatic Claude/ClaudeCode cache behavior.
6. Prefer explicit `cachedContent` reuse in Gemini workflows when available.

## Usage examples

Round-robin + cache affinity:

```toml
[channels.settings]
credential_round_robin_enabled = true
credential_cache_affinity_enabled = true
```

Round-robin without affinity:

```toml
[channels.settings]
credential_round_robin_enabled = true
credential_cache_affinity_enabled = false
```

No round-robin (sticky smallest-id available credential):

```toml
[channels.settings]
credential_round_robin_enabled = false
```
