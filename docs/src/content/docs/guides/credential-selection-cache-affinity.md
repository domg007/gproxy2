---
title: Credential Selection and Cache Affinity
description: Pick modes, internal cache-affinity pool design, hit judgment, and OpenAI/Claude/Gemini cache-hit practices.
---

## Why this page exists

When a provider has multiple credentials, cache hit rate and credential selection are coupled:

- If cache-related requests keep jumping across credentials, upstream cache hit rate usually drops.
- If all traffic is hard-pinned to one credential, throughput and fault tolerance drop.

GPROXY uses pick modes plus an internal cache-affinity pool to balance this.

## Pick mode configuration

Configure these fields in `channels.settings`:

- `credential_round_robin_enabled` (default `true`)
- `credential_cache_affinity_enabled` (default `true`)

Effective modes:

| `credential_round_robin_enabled` | `credential_cache_affinity_enabled` | Effective mode | Behavior |
|---|---|---|---|
| `false` | `false/true` | `StickyNoCache` | no round-robin, no affinity pool, always pick the smallest available credential id |
| `true` | `true` | `RoundRobinWithCache` | randomized selection among eligible credentials + cache affinity pool |
| `true` | `false` | `RoundRobinNoCache` | randomized selection among eligible credentials, no affinity pool |

Notes:

- `StickyWithCache` is intentionally not supported anymore.
- If round-robin is disabled, cache affinity is forced off.
- Legacy `credential_pick_mode` and `cache_affinity_enabled` are still parsed for backward compatibility.

## Internal cache-affinity pool design

GPROXY keeps an in-memory affinity map:

- key: `"{channel}::{affinity_key}"`
- value: `{ credential_id, expires_at }`
- scope: process-local (not persisted; no cross-instance sharing)

Selection flow (only in `RoundRobinWithCache`):

1. Build a `CacheAffinityHint` from request body/protocol.
2. Query affinity map by scoped key.
3. If record exists, not expired, and credential is currently eligible, pick that credential first.
4. Otherwise, pick randomly from eligible credentials.
5. On success, bind/update affinity record with TTL.
6. If a request was picked by affinity but failed and needs retry, clear that affinity and continue with other eligible credentials.

Eligibility still respects health/cooldown state, so affinity does not override dead/partial cooldown rules.

## Internal cache-affinity hit judgment

Two conditions must both pass:

1. A stable `affinity_key` can be derived from the request.
2. The map has a non-expired record and the mapped credential is still eligible.

If either fails, it falls back to normal randomized credential selection.

## Affinity key and TTL derivation by protocol in GPROXY

Current derivation logic in `retry.rs`:

### OpenAI-style (`/v1/responses`, `/v1/chat/completions`)

- key priority:
  - `prompt_cache_key` when present and non-empty
  - otherwise SHA-256 of the request JSON body
- TTL:
  - `24h` when `prompt_cache_retention == "24h"`
  - otherwise `5m`

### Claude-style (`/v1/messages`)

- key: SHA-256 of the request JSON body
- TTL:
  - `1h` when top-level `cache_control.ttl == "1h"`
  - otherwise `5m`

### Gemini-style (`:generateContent`, `:streamGenerateContent`)

- key priority:
  - `cachedContent` field value when present and non-empty
  - otherwise SHA-256 of `{ model, body }`
- TTL:
  - currently fixed to `5m` in GPROXY affinity map

## Upstream cache mechanisms (provider-side, not GPROXY logic)

### OpenAI

- Prompt cache is prefix-oriented.
- APIs may expose cache controls/keys depending on endpoint.
- Stable prefix, stable model, and stable tool/system sections usually improve hit rate.

### Claude

- Supports explicit block-level cache breakpoints.
- Supports automatic top-level cache control on Messages API.
- TTL is typically `5m`, with optional `1h` where supported.

### Gemini

- Uses Context Caching / `cachedContent` style resources.
- Reusing the same cached content handle usually yields better hit rate.
- GPROXY currently routes Gemini generation methods; it does not provide a dedicated helper API for creating cache resources.

## Practical ways to improve cache hit rate

1. Keep prefix content byte-stable: system prompt, tools, long context ordering, and model name.
2. Use `RoundRobinWithCache` for cache-sensitive traffic.
3. Avoid frequent credential churn during short cache windows.
4. Separate workloads with very different prompts into different channels/providers.
5. For Claude/ClaudeCode, enable `enable_top_level_cache_control` only when you want automatic cache behavior.
6. For Gemini, prefer explicit `cachedContent` reuse if your upstream workflow supports creating it.

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

No round-robin (sticky smallest-id credential, no affinity pool):

```toml
[channels.settings]
credential_round_robin_enabled = false
```
