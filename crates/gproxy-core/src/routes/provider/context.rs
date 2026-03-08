use std::sync::{Arc, Mutex};

use gproxy_middleware::{OperationFamily, ProtocolKind};
use gproxy_provider::{ChannelId, ProviderDefinition, UpstreamRequestMeta};

use crate::AppState;

use super::recording::{
    enqueue_stream_usage_event_with_estimate, enqueue_upstream_request_event_from_meta,
};
use super::{BODY_CAPTURE_LIMIT_BYTES, UpstreamResponseMeta};

#[derive(Debug, Clone, Copy)]
pub(super) struct RequestAuthContext {
    pub(super) user_id: i64,
    pub(super) user_key_id: i64,
    pub(super) downstream_trace_id: Option<i64>,
    pub(super) forced_credential_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub(super) struct UsageRequestContext {
    pub(super) operation: OperationFamily,
    pub(super) protocol: ProtocolKind,
    pub(super) model: Option<String>,
    pub(super) body_for_estimate: Option<Vec<u8>>,
}

impl UsageRequestContext {
    pub(super) const fn operation(&self) -> OperationFamily {
        self.operation
    }

    pub(super) const fn protocol(&self) -> ProtocolKind {
        self.protocol
    }
}

#[derive(Clone)]
pub(super) struct UpstreamStreamRecordContext {
    pub(super) state: Arc<AppState>,
    pub(super) channel: ChannelId,
    pub(super) provider: ProviderDefinition,
    pub(super) auth: RequestAuthContext,
    pub(super) request: UsageRequestContext,
    pub(super) provider_id: Option<i64>,
    pub(super) credential_id: Option<i64>,
    pub(super) request_meta: Option<UpstreamRequestMeta>,
    pub(super) response_status: Option<u16>,
    pub(super) response_headers: Vec<(String, String)>,
    pub(super) stream_usage: Option<gproxy_middleware::UsageHandle>,
    pub(super) record_upstream_event: bool,
    pub(super) record_stream_usage_event: bool,
}

#[derive(Default)]
struct UpstreamStreamRecordState {
    captured: Vec<u8>,
    capture_truncated: bool,
    flushed: bool,
}

pub(super) struct UpstreamStreamRecordGuard {
    context: UpstreamStreamRecordContext,
    state: Arc<Mutex<UpstreamStreamRecordState>>,
}

impl UpstreamStreamRecordGuard {
    pub(super) fn new(context: UpstreamStreamRecordContext) -> Self {
        Self {
            context,
            state: Arc::new(Mutex::new(UpstreamStreamRecordState::default())),
        }
    }

    pub(super) fn push_chunk(&self, chunk: &[u8]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.capture_truncated {
            return;
        }
        let remaining = BODY_CAPTURE_LIMIT_BYTES.saturating_sub(state.captured.len());
        if remaining > 0 {
            let take = chunk.len().min(remaining);
            state.captured.extend_from_slice(&chunk[..take]);
        }
        if state.captured.len() >= BODY_CAPTURE_LIMIT_BYTES {
            state.capture_truncated = true;
        }
    }

    fn take_flush_payload(&self) -> Option<(UpstreamStreamRecordContext, Option<Vec<u8>>)> {
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        if state.flushed {
            return None;
        }
        state.flushed = true;
        let response_body =
            (!state.captured.is_empty()).then(|| std::mem::take(&mut state.captured));
        Some((self.context.clone(), response_body))
    }

    pub(super) async fn flush_now(&self) {
        if let Some((context, response_body)) = self.take_flush_payload() {
            let response_body_for_usage = response_body.clone();
            if context.record_upstream_event {
                enqueue_upstream_request_event_from_meta(
                    context.state.as_ref(),
                    context.auth.downstream_trace_id,
                    context.provider_id,
                    context.credential_id,
                    context.request_meta.as_ref(),
                    UpstreamResponseMeta {
                        status: context.response_status,
                        headers: context.response_headers.as_slice(),
                        body: response_body,
                    },
                )
                .await;
            }
            if context.record_stream_usage_event {
                let stream_usage = context
                    .stream_usage
                    .as_ref()
                    .and_then(|handle| handle.latest());
                enqueue_stream_usage_event_with_estimate(
                    &context,
                    response_body_for_usage.as_deref().unwrap_or(&[]),
                    stream_usage,
                )
                .await;
            }
        }
    }
}

impl Drop for UpstreamStreamRecordGuard {
    fn drop(&mut self) {
        let Some((context, response_body)) = self.take_flush_payload() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let response_body_for_usage = response_body.clone();
            if context.record_upstream_event {
                enqueue_upstream_request_event_from_meta(
                    context.state.as_ref(),
                    context.auth.downstream_trace_id,
                    context.provider_id,
                    context.credential_id,
                    context.request_meta.as_ref(),
                    UpstreamResponseMeta {
                        status: context.response_status,
                        headers: context.response_headers.as_slice(),
                        body: response_body,
                    },
                )
                .await;
            }
            if context.record_stream_usage_event {
                let stream_usage = context
                    .stream_usage
                    .as_ref()
                    .and_then(|handle| handle.latest());
                enqueue_stream_usage_event_with_estimate(
                    &context,
                    response_body_for_usage.as_deref().unwrap_or(&[]),
                    stream_usage,
                )
                .await;
            }
        });
    }
}
