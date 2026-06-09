//! Streaming response tail (§6.4, D4). Native-only. Holds ONLY the body-side
//! conversion invoked by `failover` when materializing a streaming attempt — it
//! does not iterate candidates or call `classify`.

use crate::http::client::RespStream;
use crate::pipeline::outcome::ByteStream;

/// Convert a per-attempt streaming body source into the executor's `ByteStream`.
/// `RespStream` and `ByteStream` are the SAME typedef (`Item =
/// Result<Bytes, ClientError>`, D1), so this is the identity in M1 — it exists
/// as a named seam so M2 can splice per-frame transform here without touching
/// the failover loop.
pub fn into_byte_stream(s: RespStream) -> ByteStream {
    s
}
