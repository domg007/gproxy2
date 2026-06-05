# Transform Layer

`src/transform` owns provider-to-provider conversion. `src/protocol` owns
provider wire models only.

## Rules

- Organize transforms by `Operation` / `OperationGroup`, not provider family.
- Keep transforms pairwise. Do not introduce a unified IR.
- Same-kind passthrough bypasses this layer.
- Pair files own real provider field differences.
- `common/` may only contain mechanical helpers:
  - SSE framing
  - role classification
  - tool id/result helpers
  - usage arithmetic
  - error construction helpers
  - metadata/extra-field preservation helpers
- Unknown provider fields may move through only when the target wire shape has
  a safe `extra` or metadata location. Otherwise return `TransformError::LossyField`.

## Pair Files

Pair modules expose typed functions as they are implemented:

```rust
pub fn request(input: SourceRequest, ctx: &TransformContext) -> Result<TargetRequest, TransformError>;
pub fn response(input: SourceResponse, ctx: &TransformContext) -> Result<TargetResponse, TransformError>;
pub fn stream_event(input: SourceStreamEvent, ctx: &TransformContext) -> Result<TargetStreamEvent, TransformError>;
```

Only define functions that exist for that operation pair.

If a pair grows past roughly 400-500 lines, split only that pair into:

```text
pair_name/
  mod.rs
  request.rs
  response.rs
  stream.rs
  tools.rs
```
