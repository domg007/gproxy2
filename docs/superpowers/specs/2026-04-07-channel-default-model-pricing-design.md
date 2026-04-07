# Channel Default Model Pricing Design

## Goal

Allow billing to fall back to a channel-local default pricing entry when:

- a concrete model does not have an exact pricing match
- or the exact model exists but does not provide the pricing field needed for the current billing path

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

Billing currently treats model pricing as a row-level lookup.

That causes two failure modes:

1. If a request uses a model that is valid for routing and execution but absent from the pricing table:

- `estimate_billing(...)` returns `None`
- downstream usage recording falls back to `cost = 0.0`

2. If an exact model row exists but has only partial pricing:

- missing `price_each_call` does not fall back to `default`
- missing mode-specific tier sets do not fall back to `default`
- missing tool prices do not fall back to `default`

That makes sparse exact rows unintentionally suppress the channel default and forces operators to duplicate pricing data across many model rows.

This is separate from the admin-side `models.price_tiers_json` parsing issue. The live billing path uses SDK channel pricing tables, not the admin `models` table.

## Approved Design

### 1. Resolve Exact Row and `default` Row Independently

Billing keeps exact model lookup, but stops treating that as the only pricing source.

For each billing request, resolve:

- `exact_model`: the row whose `model_id == context.model_id`
- `default_model`: the row whose `model_id == "default"`

If neither exists, billing still returns `None`.

This remains channel-local because each provider runtime already owns its own `model_pricing()` slice through the concrete channel implementation.

There is no global shared default across channels.

### 2. Field-Level Fallback for Per-Call Pricing

Per-call pricing is selected by field-level precedence, not row-level precedence.

Resolution order:

- `BillingMode::Default`
  - `exact_model.price_each_call`
  - `default_model.price_each_call`
- `BillingMode::Flex`
  - `exact_model.flex_price_each_call`
  - `exact_model.price_each_call`
  - `default_model.flex_price_each_call`
  - `default_model.price_each_call`
- `BillingMode::Scale`
  - `exact_model.scale_price_each_call`
  - `exact_model.price_each_call`
  - `default_model.scale_price_each_call`
  - `default_model.price_each_call`
- `BillingMode::Priority`
  - `exact_model.priority_price_each_call`
  - `exact_model.price_each_call`
  - `default_model.priority_price_each_call`
  - `default_model.price_each_call`

This means an exact model row still wins when it provides the relevant field, but a missing field may fall through to `default`.

### 3. Field-Level Fallback for Tier Sets

Tier selection also uses field-level precedence.

Resolution order:

- `BillingMode::Default`
  - `exact_model.price_tiers`
  - `default_model.price_tiers`
- `BillingMode::Flex`
  - `exact_model.flex_price_tiers` if non-empty
  - `exact_model.price_tiers` if non-empty
  - `default_model.flex_price_tiers` if non-empty
  - `default_model.price_tiers` if non-empty
- `BillingMode::Scale`
  - `exact_model.scale_price_tiers` if non-empty
  - `exact_model.price_tiers` if non-empty
  - `default_model.scale_price_tiers` if non-empty
  - `default_model.price_tiers` if non-empty
- `BillingMode::Priority`
  - `exact_model.priority_price_tiers` if non-empty
  - `exact_model.price_tiers` if non-empty
  - `default_model.priority_price_tiers` if non-empty
  - `default_model.price_tiers` if non-empty

Once a non-empty tier set is selected, tier matching inside that set remains unchanged.

### 4. Field-Level Fallback for Tool Pricing

Tool pricing is resolved per tool key:

- `exact_model.tool_call_prices[tool_key]`
- otherwise `default_model.tool_call_prices[tool_key]`

This allows sparse exact rows to override only the tools they care about while inheriting the rest from `default`.

### 5. No `default` Means No Change

If both the exact row and the `default` row fail to provide a given pricing field:

- that field contributes nothing to cost
- if no fields contribute anything, billing still returns `Some(BillingResult { total_cost: 0.0, ... })` when an exact or default row exists
- billing returns `None` only when no exact row and no `default` row exist

This preserves the current “no pricing row means no billing result” behavior while making sparse exact rows compose with channel defaults.

### 6. Reuse Existing Price Structure

The `default` row continues to use the same schema as any other model price row and may define:

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

## Component Design

### `sdk/gproxy-provider/src/billing.rs`

Responsibilities:

- resolve exact and default pricing rows for a billing request
- resolve per-call price, tier set, and tool prices with field-level fallback
- calculate cost from the resolved pricing fields

Required changes:

- replace row-level model selection with helpers that:
  - resolve `exact_model`
  - resolve `default_model`
  - resolve effective per-call price for the current mode
  - resolve effective tier set for the current mode
  - resolve tool-call prices by key

The cost accumulation structure should remain the same after field resolution.

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

- channels that add a `default` row gain field-level fallback pricing
- channels without that row behave exactly as before
- sparse exact rows can intentionally inherit missing fields from `default`

## Data Flow

1. request completes with usage data
2. provider runtime calls `estimate_billing(model_pricing, context, usage)`
3. billing resolves `exact_model` and `default_model`
4. billing resolves the effective per-call field for the current mode
5. billing resolves the effective tier set for the current mode
6. billing resolves each tool price from exact first, then default
7. billing computes cost from the resolved fields
8. if neither `exact_model` nor `default_model` exists, return `None`

## Error Handling

This design does not add new user-visible errors.

Behavior remains:

- exact model found with complete pricing: normal billing
- exact model found with partial pricing: missing fields inherit from `default`
- exact model missing but `default` exists: normal billing through fallback
- exact model missing and `default` missing: no billing result

The implementation should not log warnings on every fallback by default, because both unknown-model fallback and sparse-row inheritance may be intentional steady-state configurations.

## Testing Strategy

Add billing-focused tests covering:

- exact model match uses the exact field, not `default`
- missing model falls back to `default`
- exact model with missing `price_each_call` falls through to `default.price_each_call`
- exact model with missing mode-specific `price_each_call` falls through through the documented precedence chain
- exact model with empty tier set falls through to the next available tier set in the documented precedence chain
- exact model tool prices override `default`, while missing tool prices inherit from `default`
- missing exact model and missing `default` still returns `None`

## Out of Scope

The following are intentionally not part of this change:

- fixing admin-side `price_tiers_json` parse failures
- persisting channel default pricing in the database
- exposing channel default pricing through admin APIs
- automatic repair of malformed pricing rows
