//! Streaming response tail (§6.4, D4). Native-only. Holds ONLY the body-side
//! conversion invoked by `failover` when materializing a streaming attempt — it
//! does not iterate candidates or call `classify`.

use crate::http::client::RespStream;
use crate::pipeline::outcome::ByteStream;
use crate::transform::stream_adapter::SseTransformer;

/// Convert a per-attempt streaming body source into the executor's `ByteStream`
/// unchanged (passthrough attempts). `RespStream` and `ByteStream` are the SAME
/// typedef (`Item = Result<Bytes, ClientError>`, D1), so this is the identity;
/// transform attempts splice [`transform_byte_stream`] instead.
pub fn into_byte_stream(s: RespStream) -> ByteStream {
    s
}

/// Wrap a streaming attempt with per-frame cross-protocol conversion. Frames
/// are re-chunked on SSE boundaries; upstream errors are forwarded once and
/// end the stream; the inbound terminator is emitted at upstream EOF.
pub fn transform_byte_stream(s: RespStream, t: SseTransformer) -> ByteStream {
    use bytes::Bytes;
    use futures_util::StreamExt;

    struct State {
        inner: Option<RespStream>,
        t: SseTransformer,
    }

    Box::pin(futures_util::stream::unfold(
        State { inner: Some(s), t },
        |mut st| async move {
            loop {
                let inner = st.inner.as_mut()?;
                match inner.next().await {
                    Some(Ok(chunk)) => {
                        let out = st.t.push(&chunk);
                        if out.is_empty() {
                            continue; // partial frame buffered; poll again
                        }
                        return Some((Ok(Bytes::from(out)), st));
                    }
                    Some(Err(e)) => {
                        st.inner = None;
                        return Some((Err(e), st));
                    }
                    None => {
                        st.inner = None;
                        let tail = st.t.finish();
                        if tail.is_empty() {
                            return None;
                        }
                        return Some((Ok(Bytes::from(tail)), st));
                    }
                }
            }
        },
    ))
}
