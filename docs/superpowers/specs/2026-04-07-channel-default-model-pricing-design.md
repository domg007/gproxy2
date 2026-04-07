# Channel Default Model Pricing Design

## Goal

Allow billing to fall back to a channel-local default pricing entry when a concrete model does not have an exact pricing match.

The fallback entry is the existing model-price row whose `model_id` is exactly `default`.

## Scope

This design changes only runtime billing lookup inside the SDK pricing path.

It does not introduce:

- new admin APIs
- new global settings
- new database schema
- new provider config fields
- any change to `gproxy-api` model storage semantics

## Current Problem

Billing currently requires an exact `model_id` match inside a channel's `model_pricing()` table.

If a request uses a model that is valid for routing and execution but absent from that pricing table:

- `estimate_billing(...)` returns `None`
- downstream usage recording falls back to `cost = 0.0`
- operators must duplicate pricing rows for many models even when one channel-wide default would be enough

This is separate from the admin-side `models.price_tiers_json` parsing issue. The live billing path uses SDK channel pricing tables, not the admin `models` table.

## Approved Design

### 1. Exact Match First

Billing lookup keeps its current first step:

1. search the current channel pricing table for an exact `model_id == context.model_id`
2. if found, use that row without fallback

This prevents a default row from overriding a deliberately configured per-model price.

### 2. Channel-Local `default` Fallback

If no exact model price is found, billing lookup performs one additional search in the same channel pricing table:

- `model_id == "default"`

If that row exists, it becomes the effective pricing row for the request.

This fallback is channel-local because each provider runtime already owns its own `model_pricing()` slice through the concrete channel implementation.

There is no global shared default across channels.

### 3. Reuse Existing Price Structure

The `default` row uses the same schema as any other model price row and may define:

- `price_each_call`
- `price_tiers`
- `flex_price_each_call`
- `flex_price_tiers`
- `scale_price_each_call`
- `scale_price_tiers`
- `priority_price_each_call`
- `priority_price_tiers`
- `tool_call_prices`

This means:

- text generation can use token-based pricing
- image or other non-token requests can use per-call pricing
- mixed pricing continues to work without new branching logic

### 4. No Fallback on Exact Match

Fallback happens only when the concrete model is missing from the pricing table.

If the concrete model exists, billing uses it even if:

- some fields are empty
- only per-call pricing is set
- only tier pricing is set

This keeps behavior predictable and avoids silently masking partially configured model rows.

### 5. No `default` Means No Change

If neither the exact model nor the `default` row exists:

- billing still returns `None`
- usage recording still resolves to `cost = 0.0`

This preserves current behavior for channels that do not define a default pricing entry.

## Component Design

### `sdk/gproxy-provider/src/billing.rs`

Responsibilities:

- select the effective pricing row for a billing request
- calculate cost from the selected pricing row

Required changes:

- extract model-price selection into a helper or equivalent local logic
- change selection from:
  - exact match only
- to:
  - exact match first
  - otherwise `default`

The cost calculation path after model selection should remain unchanged.

### `sdk/gproxy-provider/src/store.rs`

Responsibilities:

- expose billing through the provider runtime

Expected impact:

- no interface change required
- existing provider runtime call path stays intact because the fallback happens inside `billing.rs`

### Channel Pricing Data

Responsibilities:

- optionally define `model_id = "default"` rows in built-in or custom pricing tables

Expected impact:

- channels that add a `default` row gain fallback pricing
- channels without that row behave exactly as before

## Data Flow

1. request completes with usage data
2. provider runtime calls `estimate_billing(model_pricing, context, usage)`
3. billing lookup searches for `context.model_id`
4. if found, use that row
5. otherwise search for `model_id == "default"`
6. if found, compute cost from that row
7. if not found, return `None`

## Error Handling

This design does not add new user-visible errors.

Behavior remains:

- exact model found: normal billing
- exact model missing but `default` exists: normal billing through fallback
- exact model missing and `default` missing: no billing result

The implementation should not log warnings on every fallback by default, because unknown-model billing fallback may be an intentional steady-state configuration.

## Testing Strategy

Add billing-focused tests covering:

- exact model match uses the exact row, not `default`
- missing model falls back to `default`
- `default` with only `price_each_call` computes request cost correctly
- `default` with token tiers computes usage cost correctly
- missing exact model and missing `default` still returns `None`
- mode-specific fallback still works when `default` defines flex/scale/priority prices

## Out of Scope

The following are intentionally not part of this change:

- fixing admin-side `price_tiers_json` parse failures
- persisting channel default pricing in the database
- exposing channel default pricing through admin APIs
- automatic repair of malformed pricing rows
- fallback from partially configured exact rows to `default`
