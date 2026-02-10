use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;

use gproxy_provider_core::AcquireError;
use gproxy_provider_core::Event;
use gproxy_provider_core::UnavailableReason;
use gproxy_provider_core::config::{DispatchRule, OperationKind};
use gproxy_provider_core::provider::{ByteStream, UpstreamFailure};
use gproxy_provider_core::{
    AuthRetryAction, CountTokensFn, CountTokensRequest, CountTokensResponse, Credential,
    GenerateContentRequest, GenerateContentResponse, Headers, HttpMethod, ModelGetResponse,
    ModelListResponse, Op, OutputAccumulator, Proto, ProviderConfig, ProviderError,
    ProviderRegistry, ProviderResult, Request, Response, StreamEvent, TransformContext,
    TransformError, UpstreamBody, UpstreamCtx, UpstreamEvent, UpstreamHttpRequest,
    UpstreamHttpResponse, UpstreamProvider, UsageAccumulator, UsageSummary,
    fallback_usage_with_count_tokens, header_set, usage_from_response,
};

use gproxy_transform::middleware::{
    NostreamToStream, StreamToNostream, StreamTransformer, stream_format,
};

use crate::state::{AppState, CredentialInsertInput, ProviderRuntime};
use crate::upstream_client::UpstreamClient;

use gproxy_protocol::claude::count_tokens::types::Model as ClaudeModel;
use gproxy_protocol::sse::SseParser;
use serde_json::{self, Value as JsonValue};

mod dispatch;
mod types;
mod wire;

pub use types::ProxyAuth;
pub use types::ProxyCall;

use dispatch::{GenerateMode, ResolvedCall};
use wire::{StreamDecoder, content_type_for_stream, encode_openai_chat_done, encode_stream_event};

type ProviderContext = (
    Arc<dyn UpstreamProvider>,
    Arc<ProviderRuntime>,
    ProviderConfig,
);

struct NonGenerateUnavailableInput<'a> {
    cred_id: i64,
    op: Op,
    model: Option<&'a String>,
    provider_impl: &'a dyn UpstreamProvider,
    ctx: &'a UpstreamCtx,
    config: &'a ProviderConfig,
    cred: &'a Credential,
    req_native: &'a Request,
    failure: &'a UpstreamFailure,
}

struct UpstreamEventInput<'a> {
    trace_id: Option<String>,
    auth: crate::proxy_engine::ProxyAuth,
    provider: String,
    credential_id: Option<i64>,
    internal: bool,
    attempt_no: u32,
    operation: String,
    upstream_req: &'a UpstreamHttpRequest,
    response_status: Option<u16>,
    response_headers: Option<Headers>,
    response_body: Option<Vec<u8>>,
    usage: Option<UsageSummary>,
    error_kind: Option<String>,
    error_message: Option<String>,
    transport_kind: Option<gproxy_provider_core::provider::UpstreamTransportErrorKind>,
}

#[derive(Debug, Clone)]
struct ProtocolRouteCtx {
    provider: String,
    response_model_prefix_provider: Option<String>,
}

const MAX_UPSTREAM_LOG_BODY_BYTES: usize = 50 * 1024 * 1024;

macro_rules! emit_upstream_event {
    (
        $engine:expr,
        $trace_id:expr,
        $auth:expr,
        $provider:expr,
        $credential_id:expr,
        $internal:expr,
        $attempt_no:expr,
        $operation:expr,
        $upstream_req:expr,
        $response_status:expr,
        $usage:expr,
        $error_kind:expr,
        $error_message:expr,
        $transport_kind:expr $(,)?
    ) => {
        $engine.emit_upstream_event(UpstreamEventInput {
            trace_id: $trace_id,
            auth: $auth,
            provider: $provider,
            credential_id: $credential_id,
            internal: $internal,
            attempt_no: $attempt_no,
            operation: $operation.into(),
            upstream_req: $upstream_req,
            response_status: $response_status,
            response_headers: None,
            response_body: None,
            usage: $usage,
            error_kind: $error_kind,
            error_message: $error_message,
            transport_kind: $transport_kind,
        })
    };
}

#[derive(Clone)]
pub struct ProxyEngine {
    state: Arc<AppState>,
    registry: Arc<ProviderRegistry>,
    client: Arc<dyn UpstreamClient>,
    storage: Arc<dyn gproxy_storage::Storage>,
}

impl ProxyEngine {
    pub fn new(
        state: Arc<AppState>,
        registry: Arc<ProviderRegistry>,
        client: Arc<dyn UpstreamClient>,
        storage: Arc<dyn gproxy_storage::Storage>,
    ) -> Self {
        Self {
            state,
            registry,
            client,
            storage,
        }
    }

    pub fn events(&self) -> gproxy_provider_core::EventHub {
        self.state.events.clone()
    }

    pub fn event_redact_sensitive(&self) -> bool {
        self.state.global.load().event_redact_sensitive
    }

    pub fn authenticate_user_key(&self, api_key: &str) -> Option<crate::proxy_engine::ProxyAuth> {
        let snapshot = self.state.snapshot.load();

        let key = snapshot
            .user_keys
            .iter()
            .find(|k| k.enabled && k.api_key == api_key)?;
        let user = snapshot
            .users
            .iter()
            .find(|u| u.id == key.user_id && u.enabled)?;

        Some(crate::proxy_engine::ProxyAuth {
            user_id: user.id,
            user_key_id: key.id,
            user_agent: None,
        })
    }

    pub async fn handle(&self, call: ProxyCall) -> UpstreamHttpResponse {
        match call {
            ProxyCall::OAuthStart {
                trace_id,
                auth,
                provider,
                req,
            } => self.handle_oauth_start(trace_id, auth, provider, req).await,
            ProxyCall::OAuthCallback {
                trace_id,
                auth,
                provider,
                req,
            } => {
                self.handle_oauth_callback(trace_id, auth, provider, req)
                    .await
            }
            ProxyCall::UpstreamUsage {
                trace_id,
                auth,
                provider,
                credential_id,
            } => {
                self.handle_upstream_usage(trace_id, auth, provider, credential_id)
                    .await
            }
            ProxyCall::Protocol {
                trace_id,
                auth,
                provider,
                response_model_prefix_provider,
                user_proto,
                user_op,
                req,
            } => {
                self.handle_protocol(
                    trace_id,
                    auth,
                    ProtocolRouteCtx {
                        provider,
                        response_model_prefix_provider,
                    },
                    user_proto,
                    user_op,
                    *req,
                )
                .await
            }
        }
    }

    pub fn enabled_provider_names(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .state
            .snapshot
            .load()
            .providers
            .iter()
            .filter(|row| row.enabled)
            .map(|row| row.name.clone())
            .collect();
        out.sort();
        out
    }

    async fn handle_upstream_usage(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        credential_id: i64,
    ) -> UpstreamHttpResponse {
        let (provider_impl, runtime, config) = match self.load_provider(&provider) {
            Ok(v) => v,
            Err(resp) => return resp,
        };

        let dispatch = provider_impl.dispatch_table(&config);
        if matches!(
            dispatch.rule(OperationKind::Usage),
            DispatchRule::Unsupported
        ) {
            return json_error(501, "unsupported_operation");
        }

        let mut fixed_credential = match self.resolve_usage_credential(&provider, credential_id) {
            Ok(cred) => (credential_id, cred),
            Err(resp) => return resp,
        };

        let mut attempt_no: u32 = 1;
        let mut auth_retry_used: Option<i64> = None;
        let mut provider_retry_used: Option<i64> = None;
        let fake_req = Request::ModelList(gproxy_provider_core::ModelListRequest::OpenAI(
            gproxy_protocol::openai::list_models::request::ListModelsRequest,
        ));
        loop {
            let (cred_id, cred) = fixed_credential.clone();

            let ctx = UpstreamCtx {
                trace_id: trace_id.clone(),
                user_id: Some(auth.user_id),
                user_key_id: Some(auth.user_key_id),
                user_agent: None,
                outbound_proxy: self.state.global.load().proxy.clone(),
                provider: provider.clone(),
                credential_id: Some(cred_id),
                // This is a provider-internal ability, but it still performs upstream IO.
                // Use a stable op value for logging; `operation` is recorded in events separately.
                op: Op::ModelList,
                internal: true,
                attempt_no,
            };

            let mut cred = cred;
            match provider_impl
                .upgrade_credential(&ctx, &config, &cred, &fake_req)
                .await
            {
                Ok(Some(new_cred)) => {
                    if let Err(resp) = self
                        .persist_credential_update(cred_id, &new_cred, &runtime)
                        .await
                    {
                        return resp;
                    }
                    fixed_credential = (cred_id, new_cred.clone());
                    cred = new_cred;
                }
                Ok(None) => {}
                Err(err) => return error_response_from_provider_err(&err),
            }

            let upstream_req = match provider_impl
                .build_upstream_usage(&ctx, &config, &cred)
                .await
            {
                Ok(r) => r,
                Err(err) => return error_response_from_provider_err(&err),
            };

            let resp = match self.client.send(upstream_req.clone()).await {
                Ok(r) => r,
                Err(failure) => {
                    emit_upstream_event!(
                        self,
                        trace_id.clone(),
                        auth.clone(),
                        provider.clone(),
                        Some(cred_id),
                        true,
                        attempt_no,
                        "Usage",
                        &upstream_req,
                        None,
                        None,
                        Some("transport".to_string()),
                        Some(failure_message(&failure)),
                        transport_kind_from_failure(&failure),
                    )
                    .await;
                    if provider_retry_used != Some(cred_id)
                        && let Ok(action) = provider_impl
                            .on_upstream_failure(&ctx, &config, &cred, &fake_req, &failure)
                            .await
                    {
                        match action {
                            AuthRetryAction::RetrySame => {
                                provider_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::UpdateCredential(new_cred) => {
                                if let Err(resp) = self
                                    .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                    .await
                                {
                                    return resp;
                                }
                                fixed_credential = (cred_id, (*new_cred).clone());
                                provider_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::None => {}
                        }
                    }
                    if let Some(decision) =
                        provider_impl.decide_unavailable(&ctx, &config, &cred, &fake_req, &failure)
                    {
                        self.apply_unavailable_decision(
                            runtime.clone(),
                            cred_id,
                            Op::ModelList,
                            None,
                            decision,
                        )
                        .await;
                        return failure_to_http(failure);
                    }
                    return failure_to_http(failure);
                }
            };

            let status = resp.status;
            let is_success = (200..300).contains(&status);
            if !is_success {
                // Mark unavailable if provider decides so, then retry.
                let failure = match resp_body_bytes(&resp.body) {
                    Some(body) => UpstreamFailure::Http {
                        status,
                        headers: resp.headers.clone(),
                        body,
                    },
                    None => UpstreamFailure::Http {
                        status,
                        headers: resp.headers.clone(),
                        body: Bytes::new(),
                    },
                };
                emit_upstream_event!(
                    self,
                    trace_id.clone(),
                    auth.clone(),
                    provider.clone(),
                    Some(cred_id),
                    true,
                    attempt_no,
                    "Usage",
                    &upstream_req,
                    Some(status),
                    None,
                    Some("http".to_string()),
                    Some(format!("http_status_{status}")),
                    None,
                )
                .await;
                if provider_retry_used != Some(cred_id)
                    && let Ok(action) = provider_impl
                        .on_upstream_failure(&ctx, &config, &cred, &fake_req, &failure)
                        .await
                {
                    match action {
                        AuthRetryAction::RetrySame => {
                            provider_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::UpdateCredential(new_cred) => {
                            if let Err(resp) = self
                                .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                .await
                            {
                                return resp;
                            }
                            fixed_credential = (cred_id, (*new_cred).clone());
                            provider_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::None => {}
                    }
                }
                if is_auth_failure(&failure)
                    && auth_retry_used != Some(cred_id)
                    && let Ok(action) = provider_impl
                        .on_auth_failure(&ctx, &config, &cred, &fake_req, &failure)
                        .await
                {
                    match action {
                        AuthRetryAction::RetrySame => {
                            auth_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::UpdateCredential(new_cred) => {
                            if let Err(resp) = self
                                .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                .await
                            {
                                return resp;
                            }
                            fixed_credential = (cred_id, (*new_cred).clone());
                            auth_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::None => {}
                    }
                }
                if let Some(decision) =
                    provider_impl.decide_unavailable(&ctx, &config, &cred, &fake_req, &failure)
                {
                    self.apply_unavailable_decision(
                        runtime.clone(),
                        cred_id,
                        Op::ModelList,
                        None,
                        decision,
                    )
                    .await;
                    return resp;
                }
            }

            emit_upstream_event!(
                self,
                trace_id.clone(),
                auth.clone(),
                provider.clone(),
                Some(cred_id),
                true,
                attempt_no,
                "Usage",
                &upstream_req,
                Some(resp.status),
                None,
                None,
                None,
                None,
            )
            .await;

            match provider_impl
                .on_upstream_success(&ctx, &config, &cred, &fake_req, &resp)
                .await
            {
                Ok(Some(new_cred)) => {
                    if let Err(resp) = self
                        .persist_credential_update(cred_id, &new_cred, &runtime)
                        .await
                    {
                        return resp;
                    }
                }
                Ok(None) => {}
                Err(err) => return error_response_from_provider_err(&err),
            }

            return resp;
        }
    }

    fn resolve_usage_credential(
        &self,
        provider: &str,
        credential_id: i64,
    ) -> Result<Credential, UpstreamHttpResponse> {
        let snapshot = self.state.snapshot.load();
        let Some(provider_row) = snapshot.providers.iter().find(|p| p.name == provider) else {
            return Err(json_error(404, "provider_not_found"));
        };
        let Some(row) = snapshot
            .credentials
            .iter()
            .find(|c| c.id == credential_id && c.provider_id == provider_row.id)
        else {
            return Err(json_error(404, "credential_not_found"));
        };
        if !row.enabled {
            return Err(json_error(409, "credential_disabled"));
        }
        serde_json::from_value(row.secret_json.clone())
            .map_err(|err| json_error_with(500, "credential_decode_failed", err.to_string()))
    }

    async fn handle_oauth_start(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        req: gproxy_provider_core::OAuthStartRequest,
    ) -> UpstreamHttpResponse {
        let upstream_req = internal_request(format!(
            "/{provider}/oauth{}",
            req.query
                .as_ref()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        ));
        let (provider_impl, _runtime, config) = match self.load_provider(&provider) {
            Ok(v) => v,
            Err(resp) => return resp,
        };

        let ctx = UpstreamCtx {
            trace_id: trace_id.clone(),
            user_id: Some(auth.user_id),
            user_key_id: Some(auth.user_key_id),
            user_agent: None,
            outbound_proxy: self.state.global.load().proxy.clone(),
            provider: provider.clone(),
            credential_id: None,
            op: Op::ModelList,
            internal: true,
            attempt_no: 0,
        };

        match provider_impl.oauth_start(&ctx, &config, &req) {
            Ok(resp) => {
                emit_upstream_event!(
                    self,
                    trace_id.clone(),
                    auth,
                    provider,
                    None,
                    true,
                    1,
                    "OAuthStart",
                    &upstream_req,
                    Some(resp.status),
                    None,
                    None,
                    None,
                    None,
                )
                .await;
                resp
            }
            Err(err) => {
                let resp = error_response_from_provider_err(&err);
                emit_upstream_event!(
                    self,
                    trace_id.clone(),
                    auth,
                    provider,
                    None,
                    true,
                    1,
                    "OAuthStart",
                    &upstream_req,
                    Some(resp.status),
                    None,
                    Some("provider_error".to_string()),
                    Some(err.to_string()),
                    None,
                )
                .await;
                resp
            }
        }
    }

    async fn handle_oauth_callback(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        req: gproxy_provider_core::OAuthCallbackRequest,
    ) -> UpstreamHttpResponse {
        let upstream_req = internal_request(format!(
            "/{provider}/oauth/callback{}",
            req.query
                .as_ref()
                .map(|q| format!("?{q}"))
                .unwrap_or_default()
        ));
        let (provider_impl, _runtime, config) = match self.load_provider(&provider) {
            Ok(v) => v,
            Err(resp) => return resp,
        };

        let ctx = UpstreamCtx {
            trace_id: trace_id.clone(),
            user_id: Some(auth.user_id),
            user_key_id: Some(auth.user_key_id),
            user_agent: None,
            outbound_proxy: self.state.global.load().proxy.clone(),
            provider: provider.clone(),
            credential_id: None,
            op: Op::ModelList,
            internal: true,
            attempt_no: 0,
        };

        match provider_impl.oauth_callback(&ctx, &config, &req) {
            Ok(result) => {
                if let Some(cred) = result.credential {
                    let snapshot = self.state.snapshot.load();
                    let provider_row = snapshot
                        .providers
                        .iter()
                        .find(|p| p.name == provider)
                        .cloned();
                    let Some(provider_row) = provider_row else {
                        let resp = json_error(500, "provider_not_found");
                        emit_upstream_event!(
                            self,
                            trace_id.clone(),
                            auth,
                            provider,
                            None,
                            true,
                            1,
                            "OAuthCallback",
                            &upstream_req,
                            Some(resp.status),
                            None,
                            Some("storage_error".to_string()),
                            Some("provider_not_found".to_string()),
                            None,
                        )
                        .await;
                        return resp;
                    };
                    let secret_json = match serde_json::to_value(&cred.credential) {
                        Ok(v) => v,
                        Err(err) => {
                            let resp =
                                json_error_with(500, "credential_encode_failed", err.to_string());
                            emit_upstream_event!(
                                self,
                                trace_id.clone(),
                                auth,
                                provider,
                                None,
                                true,
                                1,
                                "OAuthCallback",
                                &upstream_req,
                                Some(resp.status),
                                None,
                                Some("storage_error".to_string()),
                                Some(err.to_string()),
                                None,
                            )
                            .await;
                            return resp;
                        }
                    };
                    let settings_json = cred
                        .settings_json
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({}));

                    // If same provider+account_id exists (Codex), update; otherwise insert.
                    let existing_id = match &cred.credential {
                        gproxy_provider_core::Credential::Codex(codex) => snapshot
                            .credentials
                            .iter()
                            .find(|row| {
                                row.provider_id == provider_row.id
                                    && serde_json::from_value::<gproxy_provider_core::Credential>(
                                        row.secret_json.clone(),
                                    )
                                    .ok()
                                    .and_then(|c| match c {
                                        gproxy_provider_core::Credential::Codex(existing) => {
                                            Some(existing.account_id == codex.account_id)
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or(false)
                            })
                            .map(|row| row.id),
                        _ => None,
                    };

                    if let Some(id) = existing_id {
                        if let Err(err) = self
                            .storage
                            .update_credential(
                                id,
                                cred.name.as_deref(),
                                &settings_json,
                                &secret_json,
                            )
                            .await
                        {
                            let resp = json_error_with(500, "storage_error", err.to_string());
                            emit_upstream_event!(
                                self,
                                trace_id.clone(),
                                auth,
                                provider,
                                None,
                                true,
                                1,
                                "OAuthCallback",
                                &upstream_req,
                                Some(resp.status),
                                None,
                                Some("storage_error".to_string()),
                                Some(err.to_string()),
                                None,
                            )
                            .await;
                            return resp;
                        }
                        if let Err(err) = self
                            .state
                            .apply_credential_update(
                                id,
                                cred.name.clone(),
                                settings_json.clone(),
                                secret_json,
                            )
                            .await
                        {
                            let resp = json_error_with(500, "apply_memory_failed", err.to_string());
                            emit_upstream_event!(
                                self,
                                trace_id.clone(),
                                auth,
                                provider,
                                None,
                                true,
                                1,
                                "OAuthCallback",
                                &upstream_req,
                                Some(resp.status),
                                None,
                                Some("apply_memory_failed".to_string()),
                                Some(err.to_string()),
                                None,
                            )
                            .await;
                            return resp;
                        }
                    } else {
                        let id = match self
                            .storage
                            .insert_credential(
                                &provider,
                                cred.name.as_deref(),
                                &settings_json,
                                &secret_json,
                                true,
                            )
                            .await
                        {
                            Ok(id) => id,
                            Err(err) => {
                                let resp = json_error_with(500, "storage_error", err.to_string());
                                emit_upstream_event!(
                                    self,
                                    trace_id.clone(),
                                    auth,
                                    provider,
                                    None,
                                    true,
                                    1,
                                    "OAuthCallback",
                                    &upstream_req,
                                    Some(resp.status),
                                    None,
                                    Some("storage_error".to_string()),
                                    Some(err.to_string()),
                                    None,
                                )
                                .await;
                                return resp;
                            }
                        };
                        if let Err(err) = self
                            .state
                            .apply_credential_insert(CredentialInsertInput {
                                id,
                                provider_name: provider.clone(),
                                provider_id: provider_row.id,
                                name: cred.name.clone(),
                                settings_json,
                                secret_json,
                                enabled: true,
                            })
                            .await
                        {
                            let resp = json_error_with(500, "apply_memory_failed", err.to_string());
                            emit_upstream_event!(
                                self,
                                trace_id.clone(),
                                auth,
                                provider,
                                None,
                                true,
                                1,
                                "OAuthCallback",
                                &upstream_req,
                                Some(resp.status),
                                None,
                                Some("apply_memory_failed".to_string()),
                                Some(err.to_string()),
                                None,
                            )
                            .await;
                            return resp;
                        }
                    }
                }
                emit_upstream_event!(
                    self,
                    trace_id.clone(),
                    auth,
                    provider,
                    None,
                    true,
                    1,
                    "OAuthCallback",
                    &upstream_req,
                    Some(result.response.status),
                    None,
                    None,
                    None,
                    None,
                )
                .await;
                result.response
            }
            Err(err) => {
                let resp = error_response_from_provider_err(&err);
                emit_upstream_event!(
                    self,
                    trace_id.clone(),
                    auth,
                    provider,
                    None,
                    true,
                    1,
                    "OAuthCallback",
                    &upstream_req,
                    Some(resp.status),
                    None,
                    Some("provider_error".to_string()),
                    Some(err.to_string()),
                    None,
                )
                .await;
                resp
            }
        }
    }

    async fn handle_protocol(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        route_ctx: ProtocolRouteCtx,
        user_proto: Proto,
        user_op: Op,
        req_user: Request,
    ) -> UpstreamHttpResponse {
        let provider = route_ctx.provider;
        let response_model_prefix_provider = route_ctx.response_model_prefix_provider;
        let (provider_impl, runtime, config) = match self.load_provider(&provider) {
            Ok(v) => v,
            Err(resp) => return resp,
        };

        let dispatch = provider_impl.dispatch_table(&config);
        let Some(resolved) = dispatch::resolve_call_shape(&dispatch, user_proto, user_op) else {
            return json_error(501, "unsupported_operation");
        };

        let to_provider = TransformContext {
            src: user_proto,
            dst: resolved.provider_proto,
            src_op: user_op,
            dst_op: resolved.provider_op,
        };

        let req_native = match transform_request_maybe(&to_provider, req_user) {
            Ok(r) => r,
            Err(err) => {
                return json_error_with(400, "transform_request_failed", format!("{err:?}"));
            }
        };

        let model_for_cooldown = if is_generate_op(resolved.provider_op) {
            extract_model_from_request(&req_native)
        } else {
            None
        };

        let mut attempt_no: u32 = 1;
        let mut auth_retry_used: Option<i64> = None;
        let mut provider_retry_used: Option<i64> = None;
        loop {
            let (cred_id, cred) = match model_for_cooldown.as_deref() {
                Some(model) => match runtime.pool.acquire_for_model(&provider, model).await {
                    Ok(v) => v,
                    Err(AcquireError::ProviderUnknown) => {
                        return json_error(404, "provider_not_found");
                    }
                    Err(AcquireError::NoActiveCredentials) => {
                        return json_error(503, "no_active_credentials");
                    }
                },
                None => match runtime.pool.acquire(&provider).await {
                    Ok(v) => v,
                    Err(AcquireError::ProviderUnknown) => {
                        return json_error(404, "provider_not_found");
                    }
                    Err(AcquireError::NoActiveCredentials) => {
                        return json_error(503, "no_active_credentials");
                    }
                },
            };

            let ctx = UpstreamCtx {
                trace_id: trace_id.clone(),
                user_id: Some(auth.user_id),
                user_key_id: Some(auth.user_key_id),
                user_agent: auth.user_agent.clone(),
                outbound_proxy: self.state.global.load().proxy.clone(),
                provider: provider.clone(),
                credential_id: Some(cred_id),
                op: resolved.provider_op,
                internal: false,
                attempt_no,
            };

            let mut cred = cred;
            match provider_impl
                .upgrade_credential(&ctx, &config, &cred, &req_native)
                .await
            {
                Ok(Some(new_cred)) => {
                    if let Err(resp) = self
                        .persist_credential_update(cred_id, &new_cred, &runtime)
                        .await
                    {
                        return resp;
                    }
                    cred = new_cred;
                }
                Ok(None) => {}
                Err(err) => return error_response_from_provider_err(&err),
            }

            if let Some(local_resp) =
                match provider_impl.local_response(&ctx, &config, &cred, &req_native) {
                    Ok(v) => v,
                    Err(err) => return error_response_from_provider_err(&err),
                }
            {
                let upstream_req = local_upstream_request(&provider, resolved.provider_op);
                let status = local_resp.status;
                let is_success = (200..300).contains(&status);
                if !is_success {
                    self.emit_upstream_event(UpstreamEventInput {
                        trace_id: trace_id.clone(),
                        auth: auth.clone(),
                        provider: provider.clone(),
                        credential_id: Some(cred_id),
                        internal: false,
                        attempt_no,
                        operation: format!("{:?}", resolved.provider_op),
                        upstream_req: &upstream_req,
                        response_status: Some(status),
                        response_headers: Some(local_resp.headers.clone()),
                        response_body: resp_body_bytes(&local_resp.body).map(|body| body.to_vec()),
                        usage: None,
                        error_kind: Some("http".to_string()),
                        error_message: Some(format!("http_status_{status}")),
                        transport_kind: None,
                    })
                    .await;
                    return local_resp;
                }
                return self
                    .handle_success(
                        trace_id.clone(),
                        auth,
                        provider.clone(),
                        response_model_prefix_provider.clone(),
                        provider_impl,
                        runtime,
                        config,
                        cred_id,
                        cred,
                        attempt_no,
                        user_proto,
                        user_op,
                        resolved,
                        to_provider,
                        req_native,
                        upstream_req,
                        local_resp,
                    )
                    .await;
            }

            let upstream_req = match build_upstream_request(
                provider_impl.as_ref(),
                &ctx,
                &config,
                &cred,
                &req_native,
            )
            .await
            {
                Ok(r) => r,
                Err(err) => return error_response_from_provider_err(&err),
            };

            let resp = match self.client.send(upstream_req.clone()).await {
                Ok(r) => r,
                Err(failure) => {
                    emit_upstream_event!(
                        self,
                        trace_id.clone(),
                        auth.clone(),
                        provider.clone(),
                        Some(cred_id),
                        false,
                        attempt_no,
                        format!("{:?}", resolved.provider_op),
                        &upstream_req,
                        None,
                        None,
                        Some("transport".to_string()),
                        Some(failure_message(&failure)),
                        transport_kind_from_failure(&failure),
                    )
                    .await;
                    if provider_retry_used != Some(cred_id)
                        && let Ok(action) = provider_impl
                            .on_upstream_failure(&ctx, &config, &cred, &req_native, &failure)
                            .await
                    {
                        match action {
                            AuthRetryAction::RetrySame => {
                                provider_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::UpdateCredential(new_cred) => {
                                if let Err(resp) = self
                                    .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                    .await
                                {
                                    return resp;
                                }
                                provider_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::None => {}
                        }
                    }
                    if is_auth_failure(&failure)
                        && auth_retry_used != Some(cred_id)
                        && let Ok(action) = provider_impl
                            .on_auth_failure(&ctx, &config, &cred, &req_native, &failure)
                            .await
                    {
                        match action {
                            AuthRetryAction::RetrySame => {
                                auth_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::UpdateCredential(new_cred) => {
                                if let Err(resp) = self
                                    .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                    .await
                                {
                                    return resp;
                                }
                                auth_retry_used = Some(cred_id);
                                attempt_no += 1;
                                continue;
                            }
                            AuthRetryAction::None => {}
                        }
                    }
                    if !is_generate_op(resolved.provider_op) {
                        self.handle_non_generate_unavailable(
                            runtime.clone(),
                            NonGenerateUnavailableInput {
                                cred_id,
                                op: resolved.provider_op,
                                model: model_for_cooldown.as_ref(),
                                provider_impl: provider_impl.as_ref(),
                                ctx: &ctx,
                                config: &config,
                                cred: &cred,
                                req_native: &req_native,
                                failure: &failure,
                            },
                        )
                        .await;
                        return failure_to_http(failure);
                    }
                    if let Some(decision) = provider_impl.decide_unavailable(
                        &ctx,
                        &config,
                        &cred,
                        &req_native,
                        &failure,
                    ) {
                        self.apply_unavailable_decision(
                            runtime.clone(),
                            cred_id,
                            resolved.provider_op,
                            model_for_cooldown.as_ref(),
                            decision,
                        )
                        .await;
                        if is_retryable_failure(&failure) {
                            if !self
                                .has_retry_candidate(
                                    &runtime,
                                    &provider,
                                    model_for_cooldown.as_ref(),
                                )
                                .await
                            {
                                return failure_to_http(failure);
                            }
                            backoff_sleep(attempt_no).await;
                            attempt_no += 1;
                            continue;
                        }
                        return failure_to_http(failure);
                    }
                    return failure_to_http(failure);
                }
            };

            let status = resp.status;
            let is_success = (200..300).contains(&status);
            if !is_success {
                let failure = match resp_body_bytes(&resp.body) {
                    Some(body) => UpstreamFailure::Http {
                        status,
                        headers: resp.headers.clone(),
                        body,
                    },
                    None => UpstreamFailure::Http {
                        status,
                        headers: resp.headers.clone(),
                        body: Bytes::new(),
                    },
                };
                self.emit_upstream_event(UpstreamEventInput {
                    trace_id: trace_id.clone(),
                    auth: auth.clone(),
                    provider: provider.clone(),
                    credential_id: Some(cred_id),
                    internal: false,
                    attempt_no,
                    operation: format!("{:?}", resolved.provider_op),
                    upstream_req: &upstream_req,
                    response_status: Some(status),
                    response_headers: Some(resp.headers.clone()),
                    response_body: resp_body_bytes(&resp.body).map(|body| body.to_vec()),
                    usage: None,
                    error_kind: Some("http".to_string()),
                    error_message: Some(format!("http_status_{status}")),
                    transport_kind: None,
                })
                .await;
                if provider_retry_used != Some(cred_id)
                    && let Ok(action) = provider_impl
                        .on_upstream_failure(&ctx, &config, &cred, &req_native, &failure)
                        .await
                {
                    match action {
                        AuthRetryAction::RetrySame => {
                            provider_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::UpdateCredential(new_cred) => {
                            if let Err(resp) = self
                                .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                .await
                            {
                                return resp;
                            }
                            provider_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::None => {}
                    }
                }
                if is_auth_failure(&failure)
                    && auth_retry_used != Some(cred_id)
                    && let Ok(action) = provider_impl
                        .on_auth_failure(&ctx, &config, &cred, &req_native, &failure)
                        .await
                {
                    match action {
                        AuthRetryAction::RetrySame => {
                            auth_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::UpdateCredential(new_cred) => {
                            if let Err(resp) = self
                                .persist_credential_update(cred_id, new_cred.as_ref(), &runtime)
                                .await
                            {
                                return resp;
                            }
                            auth_retry_used = Some(cred_id);
                            attempt_no += 1;
                            continue;
                        }
                        AuthRetryAction::None => {}
                    }
                }
                if !is_generate_op(resolved.provider_op) {
                    self.handle_non_generate_unavailable(
                        runtime.clone(),
                        NonGenerateUnavailableInput {
                            cred_id,
                            op: resolved.provider_op,
                            model: model_for_cooldown.as_ref(),
                            provider_impl: provider_impl.as_ref(),
                            ctx: &ctx,
                            config: &config,
                            cred: &cred,
                            req_native: &req_native,
                            failure: &failure,
                        },
                    )
                    .await;
                    return resp;
                }
                if let Some(decision) =
                    provider_impl.decide_unavailable(&ctx, &config, &cred, &req_native, &failure)
                {
                    self.apply_unavailable_decision(
                        runtime.clone(),
                        cred_id,
                        resolved.provider_op,
                        model_for_cooldown.as_ref(),
                        decision,
                    )
                    .await;
                    if is_retryable_failure(&failure) {
                        if !self
                            .has_retry_candidate(&runtime, &provider, model_for_cooldown.as_ref())
                            .await
                        {
                            return resp;
                        }
                        backoff_sleep(attempt_no).await;
                        attempt_no += 1;
                        continue;
                    }
                    return resp;
                }
                return resp;
            }

            // Success path.
            match provider_impl
                .on_upstream_success(&ctx, &config, &cred, &req_native, &resp)
                .await
            {
                Ok(Some(new_cred)) => {
                    if let Err(resp) = self
                        .persist_credential_update(cred_id, &new_cred, &runtime)
                        .await
                    {
                        return resp;
                    }
                }
                Ok(None) => {}
                Err(err) => return error_response_from_provider_err(&err),
            }
            return self
                .handle_success(
                    trace_id.clone(),
                    auth,
                    provider.clone(),
                    response_model_prefix_provider.clone(),
                    provider_impl,
                    runtime,
                    config,
                    cred_id,
                    cred,
                    attempt_no,
                    user_proto,
                    user_op,
                    resolved,
                    to_provider,
                    req_native,
                    upstream_req,
                    resp,
                )
                .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_success(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        provider_impl: Arc<dyn UpstreamProvider>,
        runtime: Arc<ProviderRuntime>,
        config: ProviderConfig,
        cred_id: i64,
        cred: Credential,
        attempt_no: u32,
        user_proto: Proto,
        user_op: Op,
        resolved: ResolvedCall,
        _to_provider: TransformContext,
        req_native: Request,
        upstream_req: UpstreamHttpRequest,
        upstream_resp: UpstreamHttpResponse,
    ) -> UpstreamHttpResponse {
        let provider_proto = resolved.provider_proto;
        let provider_op = resolved.provider_op;

        match (user_op, resolved.mode) {
            // Non-stream to non-stream (includes non-generate ops and generate non-stream).
            (
                Op::ModelList
                | Op::ModelGet
                | Op::CountTokens
                | Op::GenerateContent
                | Op::ResponseGet
                | Op::ResponseDelete
                | Op::ResponseCancel
                | Op::ResponseListInputItems
                | Op::ResponseCompact
                | Op::MemoryTraceSummarize,
                GenerateMode::Same,
            ) => {
                self.handle_nonstream_response(
                    trace_id,
                    auth,
                    provider,
                    response_model_prefix_provider,
                    provider_impl,
                    runtime,
                    config,
                    cred_id,
                    cred,
                    attempt_no,
                    user_proto,
                    user_op,
                    provider_proto,
                    provider_op,
                    &req_native,
                    upstream_req,
                    upstream_resp,
                )
                .await
            }

            // Stream -> stream
            (Op::StreamGenerateContent, GenerateMode::Same) => {
                self.handle_stream_response(
                    trace_id,
                    auth,
                    provider,
                    response_model_prefix_provider,
                    provider_impl,
                    runtime,
                    config,
                    cred_id,
                    cred,
                    attempt_no,
                    user_proto,
                    provider_proto,
                    req_native,
                    upstream_req,
                    upstream_resp,
                )
                .await
            }

            // Stream -> non-stream
            (Op::GenerateContent, GenerateMode::StreamToNon) => {
                self.handle_stream_to_nonstream(
                    trace_id,
                    auth,
                    provider,
                    response_model_prefix_provider,
                    provider_impl,
                    runtime,
                    config,
                    cred_id,
                    cred,
                    attempt_no,
                    user_proto,
                    provider_proto,
                    req_native,
                    upstream_req,
                    upstream_resp,
                )
                .await
            }

            // Non-stream -> stream
            (Op::StreamGenerateContent, GenerateMode::NonToStream) => {
                self.handle_nonstream_to_stream(
                    trace_id,
                    auth,
                    provider,
                    response_model_prefix_provider,
                    provider_impl,
                    runtime,
                    config,
                    cred_id,
                    cred,
                    attempt_no,
                    user_proto,
                    provider_proto,
                    req_native,
                    upstream_req,
                    upstream_resp,
                )
                .await
            }

            _ => json_error(500, "invalid_dispatch_state"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_nonstream_response(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        provider_impl: Arc<dyn UpstreamProvider>,
        _runtime: Arc<ProviderRuntime>,
        config: ProviderConfig,
        cred_id: i64,
        cred: Credential,
        attempt_no: u32,
        user_proto: Proto,
        user_op: Op,
        provider_proto: Proto,
        provider_op: Op,
        _req_native: &Request,
        upstream_req: UpstreamHttpRequest,
        upstream_resp: UpstreamHttpResponse,
    ) -> UpstreamHttpResponse {
        let Some(body) = resp_body_bytes(&upstream_resp.body) else {
            return json_error(502, "upstream_body_missing");
        };
        let body = if needs_internal_unwrap(&provider, provider_proto, provider_op) {
            match unwrap_internal_json_bytes(&provider, &body) {
                Ok(bytes) => bytes,
                Err(err) => return json_error_with(502, "unwrap_internal_failed", err),
            }
        } else {
            body
        };
        let ctx = UpstreamCtx {
            trace_id: trace_id.clone(),
            user_id: Some(auth.user_id),
            user_key_id: Some(auth.user_key_id),
            user_agent: auth.user_agent.clone(),
            outbound_proxy: self.state.global.load().proxy.clone(),
            provider: provider.clone(),
            credential_id: Some(cred_id),
            op: provider_op,
            internal: false,
            attempt_no,
        };
        let body = match provider_impl.normalize_nonstream_response(
            &ctx,
            &config,
            &cred,
            provider_proto,
            provider_op,
            _req_native,
            body,
        ) {
            Ok(body) => body,
            Err(err) => return error_response_from_provider_err(&err),
        };

        let resp_native = match decode_response(provider_proto, provider_op, &body) {
            Ok(r) => r,
            Err(err) => return json_error_with(502, "decode_response_failed", err.to_string()),
        };

        // Generate usage only for generate ops.
        let usage = if matches!(user_op, Op::GenerateContent) {
            resp_native_generate_usage(provider_proto, &resp_native)
        } else {
            None
        };

        self.emit_upstream_event(UpstreamEventInput {
            trace_id: trace_id.clone(),
            auth,
            provider: provider.clone(),
            credential_id: Some(cred_id),
            internal: false,
            attempt_no,
            operation: format!("{provider_op:?}"),
            upstream_req: &upstream_req,
            response_status: Some(upstream_resp.status),
            response_headers: Some(upstream_resp.headers.clone()),
            response_body: Some(body.to_vec()),
            usage: usage.clone(),
            error_kind: None,
            error_message: None,
            transport_kind: None,
        })
        .await;

        let to_user = TransformContext {
            src: provider_proto,
            dst: user_proto,
            src_op: user_op,
            dst_op: user_op,
        };
        let resp_user = match transform_response_maybe(&to_user, resp_native) {
            Ok(r) => r,
            Err(err) => {
                return json_error_with(500, "transform_response_failed", format!("{err:?}"));
            }
        };
        let resp_user =
            maybe_prefix_model_in_response(resp_user, response_model_prefix_provider.as_deref());

        let out_bytes = match encode_response(user_proto, user_op, &resp_user) {
            Ok(b) => b,
            Err(err) => return json_error_with(500, "encode_response_failed", err.to_string()),
        };

        let mut headers = upstream_resp.headers.clone();
        header_set(&mut headers, "content-type", "application/json");
        UpstreamHttpResponse {
            status: upstream_resp.status,
            headers,
            body: UpstreamBody::Bytes(out_bytes),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_stream_response(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        provider_impl: Arc<dyn UpstreamProvider>,
        _runtime: Arc<ProviderRuntime>,
        config: ProviderConfig,
        cred_id: i64,
        cred: Credential,
        attempt_no: u32,
        user_proto: Proto,
        provider_proto: Proto,
        req_native: Request,
        upstream_req: UpstreamHttpRequest,
        upstream_resp: UpstreamHttpResponse,
    ) -> UpstreamHttpResponse {
        let UpstreamBody::Stream(rx_in) = upstream_resp.body else {
            return json_error(502, "expected_stream_body");
        };
        let rx_in = if needs_internal_stream_unwrap(&provider, provider_proto) {
            map_internal_gemini_stream(&provider, rx_in)
        } else {
            rx_in
        };
        let format = match stream_format(provider_proto) {
            Some(f) => f,
            None => return json_error(500, "invalid_stream_proto"),
        };

        // Native Gemini stream passthrough.
        //
        // Protocol-level rule only:
        // - If downstream asks `alt=sse`, keep SSE framing.
        // - Otherwise prefer passthrough unless upstream is explicitly SSE, in which
        //   case we decode/encode to emit Gemini NDJSON for default downstream shape.
        let passthrough_native_gemini = user_proto == Proto::Gemini
            && provider_proto == Proto::Gemini
            && should_passthrough_native_gemini_stream(&req_native, &upstream_resp.headers);
        if passthrough_native_gemini {
            let (tx_out, rx_out) = tokio::sync::mpsc::channel::<Bytes>(32);
            let events = self.state.events.clone();
            let trace_id2 = trace_id.clone();
            let auth2 = auth;
            let provider2 = provider.clone();
            let upstream_req2 = upstream_req.clone();
            let (upstream_path, upstream_query) = split_path_query(&upstream_req.url);
            let upstream_resp_headers = upstream_resp.headers.clone();
            let redact_sensitive = self.state.global.load().event_redact_sensitive;
            let status = upstream_resp.status;

            tokio::spawn(async move {
                let mut rx_in = rx_in;
                let mut response_body = Vec::new();
                let mut error_kind: Option<String> = None;
                let mut error_message: Option<String> = None;
                while let Some(chunk) = rx_in.recv().await {
                    append_capped(
                        &mut response_body,
                        chunk.as_ref(),
                        MAX_UPSTREAM_LOG_BODY_BYTES,
                    );
                    if tx_out.send(chunk).await.is_err() {
                        error_kind = Some("stream_forward_error".to_string());
                        error_message = Some("downstream_stream_closed".to_string());
                        break;
                    }
                }
                events
                    .emit(Event::Upstream(UpstreamEvent {
                        trace_id: trace_id2,
                        at: SystemTime::now(),
                        user_id: Some(auth2.user_id),
                        user_key_id: Some(auth2.user_key_id),
                        provider: provider2,
                        credential_id: Some(cred_id),
                        internal: false,
                        attempt_no,
                        operation: format!("{:?}", Op::StreamGenerateContent),
                        request_method: upstream_req2.method.as_str().to_string(),
                        request_headers: maybe_redact_headers(
                            upstream_req2.headers.clone(),
                            redact_sensitive,
                        ),
                        request_path: upstream_path,
                        request_query: maybe_redact_query(upstream_query, redact_sensitive),
                        request_body: if redact_sensitive {
                            None
                        } else {
                            upstream_req2.body.clone().map(|b| b.to_vec())
                        },
                        response_status: Some(status),
                        response_headers: maybe_redact_headers(
                            upstream_resp_headers.clone(),
                            redact_sensitive,
                        ),
                        response_body: if redact_sensitive {
                            None
                        } else {
                            Some(response_body)
                        },
                        usage: None,
                        error_kind,
                        error_message,
                        transport_kind: None,
                    }))
                    .await;
            });

            return UpstreamHttpResponse {
                status: upstream_resp.status,
                headers: upstream_resp.headers,
                body: UpstreamBody::Stream(rx_out),
            };
        }

        let (tx_out, rx_out) = tokio::sync::mpsc::channel::<Bytes>(32);

        let events = self.state.events.clone();
        let client = self.client.clone();
        let provider_impl2 = provider_impl.clone();
        let config2 = config.clone();
        let cred2 = cred.clone();
        let trace_id2 = trace_id.clone();
        let auth2 = auth;
        let provider2 = provider.clone();
        let outbound_proxy2 = self.state.global.load().proxy.clone();
        let upstream_req2 = upstream_req.clone();
        let (upstream_path, upstream_query) = split_path_query(&upstream_req.url);
        let upstream_resp_headers = upstream_resp.headers.clone();
        let redact_sensitive = self.state.global.load().event_redact_sensitive;
        let status = upstream_resp.status;
        let prefix_provider = response_model_prefix_provider;

        tokio::spawn(async move {
            let mut decoder = StreamDecoder::new(provider_proto, format);
            let mut usage_acc = UsageAccumulator::new(provider_proto);
            let mut out_acc = OutputAccumulator::new(provider_proto);
            let mut response_body = Vec::new();
            let mut error_kind: Option<String> = None;
            let mut error_message: Option<String> = None;
            // For same-proto OpenAI streams, prefer raw passthrough to avoid dropping
            // forward-compatible events during decode/re-encode.
            let passthrough_raw = provider_proto == user_proto
                && user_proto != Proto::Gemini
                && prefix_provider.is_none();

            let mut transformer = if provider_proto == user_proto {
                None
            } else {
                let ctx = TransformContext {
                    src: provider_proto,
                    dst: user_proto,
                    src_op: Op::StreamGenerateContent,
                    dst_op: Op::StreamGenerateContent,
                };
                StreamTransformer::new(&ctx).ok()
            };

            // Extract provider-native generate request for fallback counting.
            let input_req = match &req_native {
                Request::GenerateContent(GenerateContentRequest::Claude(r)) => {
                    Some(GenerateContentRequest::Claude(r.clone()))
                }
                Request::GenerateContent(GenerateContentRequest::OpenAIChat(r)) => {
                    Some(GenerateContentRequest::OpenAIChat(r.clone()))
                }
                Request::GenerateContent(GenerateContentRequest::OpenAIResponse(r)) => {
                    Some(GenerateContentRequest::OpenAIResponse(r.clone()))
                }
                Request::GenerateContent(GenerateContentRequest::Gemini(r)) => {
                    Some(GenerateContentRequest::Gemini(r.clone()))
                }
                Request::GenerateContent(GenerateContentRequest::GeminiStream(r)) => {
                    Some(GenerateContentRequest::GeminiStream(r.clone()))
                }
                _ => None,
            };

            let mut rx_in = rx_in;
            'stream_loop: while let Some(chunk) = rx_in.recv().await {
                append_capped(
                    &mut response_body,
                    chunk.as_ref(),
                    MAX_UPSTREAM_LOG_BODY_BYTES,
                );
                if passthrough_raw {
                    for ev in decoder.push_bytes(&chunk) {
                        let _ = usage_acc.push(&ev);
                        out_acc.push(&ev);
                    }
                    if tx_out.send(chunk).await.is_err() {
                        error_kind = Some("stream_forward_error".to_string());
                        error_message = Some("downstream_stream_closed".to_string());
                        break 'stream_loop;
                    }
                    continue;
                }

                for ev in decoder.push_bytes(&chunk) {
                    let _ = usage_acc.push(&ev);
                    out_acc.push(&ev);

                    let mut out_events: Vec<StreamEvent> = Vec::new();
                    if let Some(t) = transformer.as_mut() {
                        match t.push(ev) {
                            Ok(mut v) => out_events.append(&mut v),
                            Err(err) => {
                                error_kind = Some("stream_transform_error".to_string());
                                error_message = Some(format!("{err:?}"));
                                break 'stream_loop;
                            }
                        }
                    } else {
                        out_events.push(ev);
                    }

                    for out_ev in out_events {
                        let out_ev =
                            maybe_prefix_model_in_stream_event(out_ev, prefix_provider.as_deref());
                        if let Some(bytes) = encode_stream_event(user_proto, &out_ev)
                            && tx_out.send(bytes).await.is_err()
                        {
                            error_kind = Some("stream_forward_error".to_string());
                            error_message = Some("downstream_stream_closed".to_string());
                            break 'stream_loop;
                        }
                    }
                }
            }

            if error_kind.is_none() {
                for ev in decoder.finish() {
                    let _ = usage_acc.push(&ev);
                    out_acc.push(&ev);
                    if passthrough_raw {
                        continue;
                    }

                    let mut out_events: Vec<StreamEvent> = Vec::new();
                    if let Some(t) = transformer.as_mut() {
                        match t.push(ev) {
                            Ok(mut v) => out_events.append(&mut v),
                            Err(err) => {
                                error_kind = Some("stream_transform_error".to_string());
                                error_message = Some(format!("{err:?}"));
                                break;
                            }
                        }
                    } else {
                        out_events.push(ev);
                    }

                    for out_ev in out_events {
                        let out_ev =
                            maybe_prefix_model_in_stream_event(out_ev, prefix_provider.as_deref());
                        if let Some(bytes) = encode_stream_event(user_proto, &out_ev)
                            && tx_out.send(bytes).await.is_err()
                        {
                            error_kind = Some("stream_forward_error".to_string());
                            error_message = Some("downstream_stream_closed".to_string());
                            break;
                        }
                    }
                    if error_kind.is_some() {
                        break;
                    }
                }
            }

            if error_kind.is_none()
                && !passthrough_raw
                && user_proto == Proto::OpenAIChat
                && tx_out.send(encode_openai_chat_done()).await.is_err()
            {
                error_kind = Some("stream_forward_error".to_string());
                error_message = Some("downstream_stream_closed".to_string());
            }

            // Finalize usage (provider-native).
            let mut usage = usage_acc.finalize();
            if usage.is_none()
                && error_kind.is_none()
                && let Some(input_req) = input_req
            {
                let count_fn = EngineCountTokensFn {
                    provider: provider_impl2,
                    config: config2,
                    credential: cred2,
                    trace_id: trace_id2.clone(),
                    outbound_proxy: outbound_proxy2.clone(),
                    provider_name: provider2.clone(),
                    client,
                };
                if let Ok(Ok(u)) = tokio::task::spawn_blocking(move || {
                    fallback_usage_with_count_tokens(
                        provider_proto,
                        &input_req,
                        out_acc.as_str(),
                        &count_fn,
                    )
                })
                .await
                {
                    usage = Some(u)
                }
            }

            // Emit usage event (async, non-blocking for the stream itself).
            events
                .emit(Event::Upstream(UpstreamEvent {
                    trace_id: trace_id2,
                    at: SystemTime::now(),
                    user_id: Some(auth2.user_id),
                    user_key_id: Some(auth2.user_key_id),
                    provider: provider2,
                    credential_id: Some(cred_id),
                    internal: false,
                    attempt_no,
                    operation: format!("{:?}", Op::StreamGenerateContent),
                    request_method: upstream_req2.method.as_str().to_string(),
                    request_headers: maybe_redact_headers(
                        upstream_req2.headers.clone(),
                        redact_sensitive,
                    ),
                    request_path: upstream_path,
                    request_query: maybe_redact_query(upstream_query, redact_sensitive),
                    request_body: if redact_sensitive {
                        None
                    } else {
                        upstream_req2.body.clone().map(|b| b.to_vec())
                    },
                    response_status: Some(status),
                    response_headers: maybe_redact_headers(
                        upstream_resp_headers.clone(),
                        redact_sensitive,
                    ),
                    response_body: if redact_sensitive {
                        None
                    } else {
                        Some(response_body)
                    },
                    usage,
                    error_kind,
                    error_message,
                    transport_kind: None,
                }))
                .await;
        });

        let mut headers = upstream_resp.headers;
        header_set(
            &mut headers,
            "content-type",
            content_type_for_stream(user_proto),
        );
        UpstreamHttpResponse {
            status: upstream_resp.status,
            headers,
            body: UpstreamBody::Stream(rx_out),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_stream_to_nonstream(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        provider_impl: Arc<dyn UpstreamProvider>,
        _runtime: Arc<ProviderRuntime>,
        config: ProviderConfig,
        cred_id: i64,
        cred: Credential,
        attempt_no: u32,
        user_proto: Proto,
        provider_proto: Proto,
        req_native: Request,
        upstream_req: UpstreamHttpRequest,
        upstream_resp: UpstreamHttpResponse,
    ) -> UpstreamHttpResponse {
        let UpstreamBody::Stream(mut rx) = upstream_resp.body else {
            return json_error(502, "expected_stream_body");
        };

        let format = match stream_format(provider_proto) {
            Some(f) => f,
            None => return json_error(500, "invalid_stream_proto"),
        };
        let mut decoder = StreamDecoder::new(provider_proto, format);
        let mut usage_acc = UsageAccumulator::new(provider_proto);
        let mut out_acc = OutputAccumulator::new(provider_proto);
        let mut response_body = Vec::new();
        let mut completed_resp: Option<Response> = None;

        let ctx = TransformContext {
            src: provider_proto,
            dst: user_proto,
            src_op: Op::StreamGenerateContent,
            dst_op: Op::GenerateContent,
        };
        let mut s2n = match StreamToNostream::new(&ctx) {
            Ok(v) => v,
            Err(err) => {
                return json_error_with(500, "stream_to_nonstream_init_failed", format!("{err:?}"));
            }
        };

        while let Some(chunk) = rx.recv().await {
            append_capped(
                &mut response_body,
                chunk.as_ref(),
                MAX_UPSTREAM_LOG_BODY_BYTES,
            );
            for ev in decoder.push_bytes(&chunk) {
                let _ = usage_acc.push(&ev);
                out_acc.push(&ev);
                match s2n.push(ev) {
                    Ok(Some(resp)) => completed_resp = Some(resp),
                    Ok(None) => {}
                    Err(err) => {
                        return json_error_with(
                            500,
                            "stream_to_nonstream_transform_failed",
                            format!("{err:?}"),
                        );
                    }
                }
            }
        }
        for ev in decoder.finish() {
            let _ = usage_acc.push(&ev);
            out_acc.push(&ev);
            match s2n.push(ev) {
                Ok(Some(resp)) => completed_resp = Some(resp),
                Ok(None) => {}
                Err(err) => {
                    return json_error_with(
                        500,
                        "stream_to_nonstream_transform_failed",
                        format!("{err:?}"),
                    );
                }
            }
        }

        let resp_user = match completed_resp.or_else(|| s2n.finalize_on_eof().ok().flatten()) {
            Some(r) => r,
            None => return json_error(502, "stream_to_nonstream_failed"),
        };
        let resp_user =
            maybe_prefix_model_in_response(resp_user, response_model_prefix_provider.as_deref());

        let out_bytes = match encode_response(user_proto, Op::GenerateContent, &resp_user) {
            Ok(b) => b,
            Err(err) => return json_error_with(500, "encode_response_failed", err.to_string()),
        };

        // Usage (provider-native).
        let mut usage = usage_acc.finalize();
        if usage.is_none()
            && let Some(input_req) = extract_generate_request(&req_native)
        {
            let count_fn = EngineCountTokensFn {
                provider: provider_impl.clone(),
                config: config.clone(),
                credential: cred.clone(),
                trace_id: trace_id.clone(),
                outbound_proxy: self.state.global.load().proxy.clone(),
                provider_name: provider.clone(),
                client: self.client.clone(),
            };
            if let Ok(Ok(u)) = tokio::task::spawn_blocking(move || {
                fallback_usage_with_count_tokens(
                    provider_proto,
                    &input_req,
                    out_acc.as_str(),
                    &count_fn,
                )
            })
            .await
            {
                usage = Some(u)
            }
        }

        self.emit_upstream_event(UpstreamEventInput {
            trace_id,
            auth,
            provider,
            credential_id: Some(cred_id),
            internal: false,
            attempt_no,
            operation: format!("{:?}", Op::StreamGenerateContent),
            upstream_req: &upstream_req,
            response_status: Some(upstream_resp.status),
            response_headers: Some(upstream_resp.headers.clone()),
            response_body: Some(response_body),
            usage: usage.clone(),
            error_kind: None,
            error_message: None,
            transport_kind: None,
        })
        .await;

        let mut headers = upstream_resp.headers;
        header_set(&mut headers, "content-type", "application/json");
        UpstreamHttpResponse {
            status: upstream_resp.status,
            headers,
            body: UpstreamBody::Bytes(out_bytes),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_nonstream_to_stream(
        &self,
        trace_id: Option<String>,
        auth: crate::proxy_engine::ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        _provider_impl: Arc<dyn UpstreamProvider>,
        _runtime: Arc<ProviderRuntime>,
        _config: ProviderConfig,
        cred_id: i64,
        _cred: Credential,
        attempt_no: u32,
        user_proto: Proto,
        provider_proto: Proto,
        _req_native: Request,
        upstream_req: UpstreamHttpRequest,
        upstream_resp: UpstreamHttpResponse,
    ) -> UpstreamHttpResponse {
        let Some(body) = resp_body_bytes(&upstream_resp.body) else {
            return json_error(502, "upstream_body_missing");
        };
        let resp_native = match decode_response(provider_proto, Op::GenerateContent, &body) {
            Ok(r) => r,
            Err(err) => return json_error_with(502, "decode_response_failed", err.to_string()),
        };

        // Extract usage from provider non-stream response if present.
        let usage = resp_native_generate_usage(provider_proto, &resp_native);
        self.emit_upstream_event(UpstreamEventInput {
            trace_id: trace_id.clone(),
            auth,
            provider: provider.clone(),
            credential_id: Some(cred_id),
            internal: false,
            attempt_no,
            operation: format!("{:?}", Op::GenerateContent),
            upstream_req: &upstream_req,
            response_status: Some(upstream_resp.status),
            response_headers: Some(upstream_resp.headers.clone()),
            response_body: Some(body.to_vec()),
            usage: usage.clone(),
            error_kind: None,
            error_message: None,
            transport_kind: None,
        })
        .await;

        let ctx = TransformContext {
            src: provider_proto,
            dst: user_proto,
            src_op: Op::GenerateContent,
            dst_op: Op::StreamGenerateContent,
        };
        let mut n2s = match NostreamToStream::new(&ctx) {
            Ok(v) => v,
            Err(err) => {
                return json_error_with(500, "nostream_to_stream_init_failed", format!("{err:?}"));
            }
        };

        let out_events = match n2s.transform_response(resp_native) {
            Ok(v) => v,
            Err(err) => {
                return json_error_with(500, "nostream_to_stream_failed", format!("{err:?}"));
            }
        };
        let out_events: Vec<StreamEvent> = out_events
            .into_iter()
            .map(|ev| {
                maybe_prefix_model_in_stream_event(ev, response_model_prefix_provider.as_deref())
            })
            .collect();

        let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(32);
        tokio::spawn(async move {
            for ev in out_events {
                if let Some(bytes) = encode_stream_event(user_proto, &ev)
                    && tx.send(bytes).await.is_err()
                {
                    return;
                }
            }
            if user_proto == Proto::OpenAIChat {
                let _ = tx.send(encode_openai_chat_done()).await;
            }
        });

        let mut headers = upstream_resp.headers;
        header_set(
            &mut headers,
            "content-type",
            content_type_for_stream(user_proto),
        );
        UpstreamHttpResponse {
            status: upstream_resp.status,
            headers,
            body: UpstreamBody::Stream(rx),
        }
    }

    async fn apply_unavailable_decision(
        &self,
        runtime: Arc<ProviderRuntime>,
        cred_id: i64,
        op: Op,
        model: Option<&String>,
        decision: gproxy_provider_core::provider::UnavailableDecision,
    ) {
        if !is_generate_op(op) {
            if matches!(decision.reason, UnavailableReason::AuthInvalid) {
                runtime
                    .pool
                    .mark_unavailable(cred_id, decision.duration, decision.reason)
                    .await;
            }
            return;
        }
        let use_model = model.is_some()
            && is_generate_op(op)
            && matches!(
                decision.reason,
                UnavailableReason::RateLimit | UnavailableReason::ModelDisallow
            );
        if use_model {
            if let Some(model) = model {
                runtime
                    .pool
                    .mark_model_unavailable(
                        cred_id,
                        model.clone(),
                        decision.duration,
                        decision.reason,
                    )
                    .await;
            } else {
                runtime
                    .pool
                    .mark_unavailable(cred_id, decision.duration, decision.reason)
                    .await;
            }
        } else {
            runtime
                .pool
                .mark_unavailable(cred_id, decision.duration, decision.reason)
                .await;
        }
    }

    async fn persist_credential_update(
        &self,
        credential_id: i64,
        credential: &Credential,
        runtime: &Arc<ProviderRuntime>,
    ) -> Result<(), UpstreamHttpResponse> {
        let secret_json = serde_json::to_value(credential)
            .map_err(|err| json_error_with(500, "credential_encode_failed", err.to_string()))?;

        let (name, settings_json) = {
            let snapshot = self.state.snapshot.load();
            let name = snapshot
                .credentials
                .iter()
                .find(|row| row.id == credential_id)
                .and_then(|row| row.name.clone());
            let settings_json = snapshot
                .credentials
                .iter()
                .find(|row| row.id == credential_id)
                .map(|row| row.settings_json.clone())
                .unwrap_or_else(|| serde_json::json!({}));
            (name, settings_json)
        };

        if let Err(err) = self
            .storage
            .update_credential(credential_id, name.as_deref(), &settings_json, &secret_json)
            .await
        {
            return Err(json_error_with(500, "storage_error", err.to_string()));
        }

        if let Err(err) = self
            .state
            .apply_credential_update(credential_id, name, settings_json, secret_json)
            .await
        {
            return Err(json_error_with(500, "apply_memory_failed", err.to_string()));
        }

        // Keep runtime pool consistent even if snapshot row is disabled/missing.
        runtime
            .pool
            .update_credential(credential_id, credential.clone())
            .await;

        Ok(())
    }

    async fn handle_non_generate_unavailable(
        &self,
        runtime: Arc<ProviderRuntime>,
        input: NonGenerateUnavailableInput<'_>,
    ) {
        if !is_auth_failure(input.failure) {
            return;
        }
        if let Some(decision) = input.provider_impl.decide_unavailable(
            input.ctx,
            input.config,
            input.cred,
            input.req_native,
            input.failure,
        ) {
            self.apply_unavailable_decision(
                runtime,
                input.cred_id,
                input.op,
                input.model,
                decision,
            )
            .await;
        }
    }

    fn load_provider(&self, provider: &str) -> Result<ProviderContext, UpstreamHttpResponse> {
        // Respect admin-configured enabled flag from the in-memory snapshot.
        let enabled = {
            let snap = self.state.snapshot.load();
            snap.providers
                .iter()
                .find(|p| p.name == provider)
                .map(|p| p.enabled)
                .unwrap_or(false)
        };
        if !enabled {
            return Err(json_error(404, "provider_disabled"));
        }

        let runtime = {
            let map = self.state.providers.load();
            map.get(provider).cloned()
        };
        let Some(runtime) = runtime else {
            return Err(json_error(404, "provider_not_found"));
        };

        let cfg_value = runtime.config_json.load().as_ref().clone();
        let cfg: ProviderConfig = serde_json::from_value(cfg_value)
            .map_err(|err| json_error_with(500, "provider_config_invalid", err.to_string()))?;

        let provider_impl_name = provider_impl_name_from_config(&cfg);
        let Some(provider_impl) = self.registry.get(provider_impl_name) else {
            return Err(json_error(404, "provider_not_found"));
        };

        Ok((provider_impl, runtime, cfg))
    }

    async fn has_retry_candidate(
        &self,
        runtime: &Arc<ProviderRuntime>,
        provider: &str,
        model: Option<&String>,
    ) -> bool {
        match model {
            Some(model) => runtime
                .pool
                .acquire_for_model(provider, model)
                .await
                .is_ok(),
            None => runtime.pool.acquire(provider).await.is_ok(),
        }
    }

    async fn emit_upstream_event(&self, input: UpstreamEventInput<'_>) {
        let redact_sensitive = self.state.global.load().event_redact_sensitive;
        let (request_path, request_query) = split_path_query(&input.upstream_req.url);
        self.state
            .events
            .emit(Event::Upstream(UpstreamEvent {
                trace_id: input.trace_id,
                at: SystemTime::now(),
                user_id: Some(input.auth.user_id),
                user_key_id: Some(input.auth.user_key_id),
                provider: input.provider,
                credential_id: input.credential_id,
                internal: input.internal,
                attempt_no: input.attempt_no,
                operation: input.operation,
                request_method: input.upstream_req.method.as_str().to_string(),
                request_headers: maybe_redact_headers(
                    input.upstream_req.headers.clone(),
                    redact_sensitive,
                ),
                request_path,
                request_query: maybe_redact_query(request_query, redact_sensitive),
                request_body: if redact_sensitive {
                    None
                } else {
                    input.upstream_req.body.clone().map(|b| b.to_vec())
                },
                response_status: input.response_status,
                response_headers: maybe_redact_headers(
                    input.response_headers.unwrap_or_default(),
                    redact_sensitive,
                ),
                response_body: if redact_sensitive {
                    None
                } else {
                    input.response_body
                },
                usage: input.usage,
                error_kind: input.error_kind,
                error_message: input.error_message,
                transport_kind: input.transport_kind,
            }))
            .await;
    }
}

fn split_path_query(target: &str) -> (String, Option<String>) {
    if let Some(scheme_idx) = target.find("://") {
        let rest = &target[(scheme_idx + 3)..];
        if let Some(path_idx) = rest.find('/') {
            let path_and_query = &rest[path_idx..];
            if let Some(q_idx) = path_and_query.find('?') {
                return (
                    path_and_query[..q_idx].to_string(),
                    Some(path_and_query[(q_idx + 1)..].to_string()),
                );
            }
            return (path_and_query.to_string(), None);
        }
        return ("/".to_string(), None);
    }

    if let Some(q_idx) = target.find('?') {
        (
            target[..q_idx].to_string(),
            Some(target[(q_idx + 1)..].to_string()),
        )
    } else {
        (target.to_string(), None)
    }
}

fn maybe_redact_headers(mut headers: Headers, redact: bool) -> Headers {
    if !redact {
        return headers;
    }
    for (k, v) in &mut headers {
        let key = k.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "authorization" | "x-api-key" | "x-goog-api-key" | "cookie" | "set-cookie"
        ) {
            *v = "***".to_string();
        }
    }
    headers
}

fn maybe_redact_query(query: Option<String>, redact: bool) -> Option<String> {
    let q = query?;
    if !redact {
        return Some(q);
    }
    let Ok(mut pairs) = serde_urlencoded::from_str::<Vec<(String, String)>>(&q) else {
        return Some(q);
    };
    for (k, v) in &mut pairs {
        let key = k.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "key"
                | "api_key"
                | "access_token"
                | "refresh_token"
                | "authorization"
                | "session_key"
                | "code"
        ) {
            *v = "***".to_string();
        }
    }
    serde_urlencoded::to_string(pairs).ok()
}

fn provider_impl_name_from_config(cfg: &ProviderConfig) -> &'static str {
    match cfg {
        ProviderConfig::OpenAI(_) => "openai",
        ProviderConfig::Claude(_) => "claude",
        ProviderConfig::AIStudio(_) => "aistudio",
        ProviderConfig::VertexExpress(_) => "vertexexpress",
        ProviderConfig::Vertex(_) => "vertex",
        ProviderConfig::GeminiCli(_) => "geminicli",
        ProviderConfig::ClaudeCode(_) => "claudecode",
        ProviderConfig::Codex(_) => "codex",
        ProviderConfig::Antigravity(_) => "antigravity",
        ProviderConfig::Nvidia(_) => "nvidia",
        ProviderConfig::DeepSeek(_) => "deepseek",
        ProviderConfig::Custom(_) => "custom",
    }
}

// ---- CountTokens adapter for fallback usage counting ----

struct EngineCountTokensFn {
    provider: Arc<dyn UpstreamProvider>,
    config: ProviderConfig,
    credential: Credential,
    trace_id: Option<String>,
    outbound_proxy: Option<String>,
    provider_name: String,
    client: Arc<dyn UpstreamClient>,
}

impl CountTokensFn for EngineCountTokensFn {
    type Error = String;

    fn count_tokens(
        &self,
        _proto: Proto,
        req: CountTokensRequest,
    ) -> Result<CountTokensResponse, Self::Error> {
        tokio::runtime::Handle::current().block_on(async move {
            let ctx = UpstreamCtx {
                trace_id: self.trace_id.clone(),
                user_id: None,
                user_key_id: None,
                user_agent: None,
                outbound_proxy: self.outbound_proxy.clone(),
                provider: self.provider_name.clone(),
                credential_id: None,
                op: Op::CountTokens,
                internal: true,
                attempt_no: 0,
            };

            let upstream_req = match &req {
                CountTokensRequest::Claude(r) => {
                    self.provider
                        .build_claude_count_tokens(&ctx, &self.config, &self.credential, r)
                        .await
                }
                CountTokensRequest::OpenAI(r) => {
                    self.provider
                        .build_openai_input_tokens(&ctx, &self.config, &self.credential, r)
                        .await
                }
                CountTokensRequest::Gemini(r) => {
                    self.provider
                        .build_gemini_count_tokens(&ctx, &self.config, &self.credential, r)
                        .await
                }
            }
            .map_err(|e| format!("{e:?}"))?;

            let resp = self
                .client
                .send(upstream_req)
                .await
                .map_err(|e| format!("{e:?}"))?;
            if !(200..300).contains(&resp.status) {
                return Err(format!("count_tokens upstream status {}", resp.status));
            }
            let Some(body) = resp_body_bytes(&resp.body) else {
                return Err("count_tokens empty body".to_string());
            };
            decode_count_tokens_response(&req, &body).map_err(|e| e.to_string())
        })
    }
}

fn decode_count_tokens_response(
    req: &CountTokensRequest,
    body: &Bytes,
) -> Result<CountTokensResponse, serde_json::Error> {
    Ok(match req {
        CountTokensRequest::Claude(_) => {
            let resp = serde_json::from_slice::<
                gproxy_protocol::claude::count_tokens::response::CountTokensResponse,
            >(body)?;
            CountTokensResponse::Claude(resp)
        }
        CountTokensRequest::OpenAI(_) => {
            let resp = serde_json::from_slice::<
                gproxy_protocol::openai::count_tokens::response::InputTokenCountResponse,
            >(body)?;
            CountTokensResponse::OpenAI(resp)
        }
        CountTokensRequest::Gemini(_) => {
            let resp = serde_json::from_slice::<
                gproxy_protocol::gemini::count_tokens::response::CountTokensResponse,
            >(body)?;
            CountTokensResponse::Gemini(resp)
        }
    })
}

// ---- request/response helpers ----

fn transform_request_maybe(
    ctx: &TransformContext,
    req: Request,
) -> Result<Request, TransformError> {
    if ctx.src == ctx.dst && ctx.src_op == ctx.dst_op {
        return Ok(req);
    }
    gproxy_transform::middleware::transform_request(ctx, req)
}

fn transform_response_maybe(
    ctx: &TransformContext,
    resp: Response,
) -> Result<Response, TransformError> {
    if ctx.src == ctx.dst && ctx.src_op == ctx.dst_op {
        return Ok(resp);
    }
    gproxy_transform::middleware::transform_response(ctx, resp)
}

async fn build_upstream_request(
    provider: &dyn UpstreamProvider,
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    credential: &Credential,
    req: &Request,
) -> ProviderResult<UpstreamHttpRequest> {
    match req {
        Request::ModelList(req) => match req {
            gproxy_provider_core::ModelListRequest::Claude(r) => {
                provider
                    .build_claude_models_list(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::ModelListRequest::OpenAI(r) => {
                provider
                    .build_openai_models_list(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::ModelListRequest::Gemini(r) => {
                provider
                    .build_gemini_models_list(ctx, config, credential, r)
                    .await
            }
        },
        Request::ModelGet(req) => match req {
            gproxy_provider_core::ModelGetRequest::Claude(r) => {
                provider
                    .build_claude_models_get(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::ModelGetRequest::OpenAI(r) => {
                provider
                    .build_openai_models_get(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::ModelGetRequest::Gemini(r) => {
                provider
                    .build_gemini_models_get(ctx, config, credential, r)
                    .await
            }
        },
        Request::CountTokens(req) => match req {
            gproxy_provider_core::CountTokensRequest::Claude(r) => {
                provider
                    .build_claude_count_tokens(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::CountTokensRequest::OpenAI(r) => {
                provider
                    .build_openai_input_tokens(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::CountTokensRequest::Gemini(r) => {
                provider
                    .build_gemini_count_tokens(ctx, config, credential, r)
                    .await
            }
        },
        Request::GenerateContent(req) => match req {
            gproxy_provider_core::GenerateContentRequest::Claude(r) => {
                provider
                    .build_claude_messages(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::GenerateContentRequest::OpenAIChat(r) => {
                provider.build_openai_chat(ctx, config, credential, r).await
            }
            gproxy_provider_core::GenerateContentRequest::OpenAIResponse(r) => {
                provider
                    .build_openai_responses(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::GenerateContentRequest::Gemini(r) => {
                provider
                    .build_gemini_generate(ctx, config, credential, r)
                    .await
            }
            gproxy_provider_core::GenerateContentRequest::GeminiStream(r) => {
                provider
                    .build_gemini_generate_stream(ctx, config, credential, r)
                    .await
            }
        },
        Request::ResponseGet(req) => match req {
            gproxy_provider_core::ResponseGetRequest::OpenAI(r) => {
                provider
                    .build_openai_response_get(ctx, config, credential, r)
                    .await
            }
        },
        Request::ResponseDelete(req) => match req {
            gproxy_provider_core::ResponseDeleteRequest::OpenAI(r) => {
                provider
                    .build_openai_response_delete(ctx, config, credential, r)
                    .await
            }
        },
        Request::ResponseCancel(req) => match req {
            gproxy_provider_core::ResponseCancelRequest::OpenAI(r) => {
                provider
                    .build_openai_response_cancel(ctx, config, credential, r)
                    .await
            }
        },
        Request::ResponseListInputItems(req) => match req {
            gproxy_provider_core::ResponseListInputItemsRequest::OpenAI(r) => {
                provider
                    .build_openai_response_list_input_items(ctx, config, credential, r)
                    .await
            }
        },
        Request::ResponseCompact(req) => match req {
            gproxy_provider_core::ResponseCompactRequest::OpenAI(r) => {
                provider
                    .build_openai_response_compact(ctx, config, credential, r)
                    .await
            }
        },
        Request::MemoryTraceSummarize(req) => match req {
            gproxy_provider_core::MemoryTraceSummarizeRequest::OpenAI(r) => {
                provider
                    .build_openai_memory_trace_summarize(ctx, config, credential, r)
                    .await
            }
        },
    }
}

fn local_upstream_request(provider: &str, op: Op) -> UpstreamHttpRequest {
    let method = match op {
        Op::ModelList | Op::ModelGet | Op::ResponseGet | Op::ResponseListInputItems => {
            HttpMethod::Get
        }
        Op::ResponseDelete => HttpMethod::Delete,
        Op::CountTokens
        | Op::GenerateContent
        | Op::StreamGenerateContent
        | Op::ResponseCancel
        | Op::ResponseCompact
        | Op::MemoryTraceSummarize => HttpMethod::Post,
    };
    UpstreamHttpRequest {
        method,
        url: format!("local://{provider}/{op:?}"),
        headers: Vec::new(),
        body: None,
        is_stream: matches!(op, Op::StreamGenerateContent),
    }
}

fn decode_response(proto: Proto, op: Op, body: &Bytes) -> Result<Response, serde_json::Error> {
    match op {
        Op::ModelList => Ok(Response::ModelList(match proto {
            Proto::Claude => ModelListResponse::Claude(serde_json::from_slice(body)?),
            Proto::OpenAI => ModelListResponse::OpenAI(serde_json::from_slice(body)?),
            Proto::Gemini => ModelListResponse::Gemini(serde_json::from_slice(body)?),
            _ => {
                return Ok(Response::ModelList(ModelListResponse::OpenAI(
                    serde_json::from_slice(body)?,
                )));
            } // unreachable
        })),
        Op::ModelGet => Ok(Response::ModelGet(match proto {
            Proto::Claude => ModelGetResponse::Claude(serde_json::from_slice(body)?),
            Proto::OpenAI => ModelGetResponse::OpenAI(serde_json::from_slice(body)?),
            Proto::Gemini => ModelGetResponse::Gemini(serde_json::from_slice(body)?),
            _ => {
                return Ok(Response::ModelGet(ModelGetResponse::OpenAI(
                    serde_json::from_slice(body)?,
                )));
            } // unreachable
        })),
        Op::CountTokens => Ok(Response::CountTokens(match proto {
            Proto::Claude => CountTokensResponse::Claude(serde_json::from_slice(body)?),
            Proto::OpenAI => CountTokensResponse::OpenAI(serde_json::from_slice(body)?),
            Proto::Gemini => CountTokensResponse::Gemini(serde_json::from_slice(body)?),
            _ => {
                return Ok(Response::CountTokens(CountTokensResponse::OpenAI(
                    serde_json::from_slice(body)?,
                )));
            } // unreachable
        })),
        Op::GenerateContent => Ok(Response::GenerateContent(match proto {
            Proto::Claude => GenerateContentResponse::Claude(serde_json::from_slice(body)?),
            Proto::OpenAIChat => GenerateContentResponse::OpenAIChat(serde_json::from_slice(body)?),
            Proto::OpenAIResponse => {
                GenerateContentResponse::OpenAIResponse(serde_json::from_slice(body)?)
            }
            Proto::Gemini => GenerateContentResponse::Gemini(serde_json::from_slice(body)?),
            Proto::OpenAI => {
                return Ok(Response::GenerateContent(
                    GenerateContentResponse::OpenAIResponse(serde_json::from_slice(body)?),
                ));
            } // unreachable
        })),
        Op::ResponseGet => Ok(Response::ResponseGet(match proto {
            Proto::OpenAI => {
                gproxy_provider_core::ResponseGetResponse::OpenAI(serde_json::from_slice(body)?)
            }
            _ => {
                return Ok(Response::ResponseGet(
                    gproxy_provider_core::ResponseGetResponse::OpenAI(serde_json::from_slice(
                        body,
                    )?),
                ));
            }
        })),
        Op::ResponseDelete => Ok(Response::ResponseDelete(match proto {
            Proto::OpenAI => {
                gproxy_provider_core::ResponseDeleteResponse::OpenAI(serde_json::from_slice(body)?)
            }
            _ => {
                return Ok(Response::ResponseDelete(
                    gproxy_provider_core::ResponseDeleteResponse::OpenAI(serde_json::from_slice(
                        body,
                    )?),
                ));
            }
        })),
        Op::ResponseCancel => Ok(Response::ResponseCancel(match proto {
            Proto::OpenAI => {
                gproxy_provider_core::ResponseCancelResponse::OpenAI(serde_json::from_slice(body)?)
            }
            _ => {
                return Ok(Response::ResponseCancel(
                    gproxy_provider_core::ResponseCancelResponse::OpenAI(serde_json::from_slice(
                        body,
                    )?),
                ));
            }
        })),
        Op::ResponseListInputItems => Ok(Response::ResponseListInputItems(match proto {
            Proto::OpenAI => gproxy_provider_core::ResponseListInputItemsResponse::OpenAI(
                serde_json::from_slice(body)?,
            ),
            _ => {
                return Ok(Response::ResponseListInputItems(
                    gproxy_provider_core::ResponseListInputItemsResponse::OpenAI(
                        serde_json::from_slice(body)?,
                    ),
                ));
            }
        })),
        Op::ResponseCompact => Ok(Response::ResponseCompact(match proto {
            Proto::OpenAI => {
                gproxy_provider_core::ResponseCompactResponse::OpenAI(serde_json::from_slice(body)?)
            }
            _ => {
                return Ok(Response::ResponseCompact(
                    gproxy_provider_core::ResponseCompactResponse::OpenAI(serde_json::from_slice(
                        body,
                    )?),
                ));
            }
        })),
        Op::MemoryTraceSummarize => Ok(Response::MemoryTraceSummarize(match proto {
            Proto::OpenAI => gproxy_provider_core::MemoryTraceSummarizeResponse::OpenAI(
                serde_json::from_slice(body)?,
            ),
            _ => {
                return Ok(Response::MemoryTraceSummarize(
                    gproxy_provider_core::MemoryTraceSummarizeResponse::OpenAI(
                        serde_json::from_slice(body)?,
                    ),
                ));
            }
        })),
        Op::StreamGenerateContent => Err(serde_json::Error::io(std::io::Error::other(
            "stream response must be decoded via stream parser",
        ))),
    }
}

fn encode_response(_proto: Proto, op: Op, resp: &Response) -> Result<Bytes, serde_json::Error> {
    let bytes = match (op, resp) {
        (Op::ModelList, Response::ModelList(r)) => match r {
            ModelListResponse::Claude(v) => serde_json::to_vec(v)?,
            ModelListResponse::OpenAI(v) => serde_json::to_vec(v)?,
            ModelListResponse::Gemini(v) => serde_json::to_vec(v)?,
        },
        (Op::ModelGet, Response::ModelGet(r)) => match r {
            ModelGetResponse::Claude(v) => serde_json::to_vec(v)?,
            ModelGetResponse::OpenAI(v) => serde_json::to_vec(v)?,
            ModelGetResponse::Gemini(v) => serde_json::to_vec(v)?,
        },
        (Op::CountTokens, Response::CountTokens(r)) => match r {
            CountTokensResponse::Claude(v) => serde_json::to_vec(v)?,
            CountTokensResponse::OpenAI(v) => serde_json::to_vec(v)?,
            CountTokensResponse::Gemini(v) => serde_json::to_vec(v)?,
        },
        (Op::GenerateContent, Response::GenerateContent(r)) => match r {
            GenerateContentResponse::Claude(v) => serde_json::to_vec(v)?,
            GenerateContentResponse::OpenAIChat(v) => serde_json::to_vec(v)?,
            GenerateContentResponse::OpenAIResponse(v) => serde_json::to_vec(v)?,
            GenerateContentResponse::Gemini(v) => serde_json::to_vec(v)?,
        },
        (Op::ResponseGet, Response::ResponseGet(r)) => match r {
            gproxy_provider_core::ResponseGetResponse::OpenAI(v) => serde_json::to_vec(v)?,
        },
        (Op::ResponseDelete, Response::ResponseDelete(r)) => match r {
            gproxy_provider_core::ResponseDeleteResponse::OpenAI(v) => serde_json::to_vec(v)?,
        },
        (Op::ResponseCancel, Response::ResponseCancel(r)) => match r {
            gproxy_provider_core::ResponseCancelResponse::OpenAI(v) => serde_json::to_vec(v)?,
        },
        (Op::ResponseListInputItems, Response::ResponseListInputItems(r)) => match r {
            gproxy_provider_core::ResponseListInputItemsResponse::OpenAI(v) => {
                serde_json::to_vec(v)?
            }
        },
        (Op::ResponseCompact, Response::ResponseCompact(r)) => match r {
            gproxy_provider_core::ResponseCompactResponse::OpenAI(v) => serde_json::to_vec(v)?,
        },
        (Op::MemoryTraceSummarize, Response::MemoryTraceSummarize(r)) => match r {
            gproxy_provider_core::MemoryTraceSummarizeResponse::OpenAI(v) => serde_json::to_vec(v)?,
        },
        _ => serde_json::to_vec(&serde_json::json!({ "error": "op_mismatch" }))?,
    };
    Ok(Bytes::from(bytes))
}

fn extract_generate_request(req: &Request) -> Option<GenerateContentRequest> {
    match req {
        Request::GenerateContent(GenerateContentRequest::Claude(r)) => {
            Some(GenerateContentRequest::Claude(r.clone()))
        }
        Request::GenerateContent(GenerateContentRequest::OpenAIChat(r)) => {
            Some(GenerateContentRequest::OpenAIChat(r.clone()))
        }
        Request::GenerateContent(GenerateContentRequest::OpenAIResponse(r)) => {
            Some(GenerateContentRequest::OpenAIResponse(r.clone()))
        }
        Request::GenerateContent(GenerateContentRequest::Gemini(r)) => {
            Some(GenerateContentRequest::Gemini(r.clone()))
        }
        Request::GenerateContent(GenerateContentRequest::GeminiStream(r)) => {
            Some(GenerateContentRequest::GeminiStream(r.clone()))
        }
        _ => None,
    }
}

fn resp_native_generate_usage(proto: Proto, resp: &Response) -> Option<UsageSummary> {
    match resp {
        Response::GenerateContent(r) => usage_from_response(proto, r),
        _ => None,
    }
}

fn append_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) -> bool {
    if buf.len() >= cap {
        return true;
    }
    let remaining = cap.saturating_sub(buf.len());
    let take = remaining.min(chunk.len());
    buf.extend_from_slice(&chunk[..take]);
    take < chunk.len()
}

fn resp_body_bytes(body: &UpstreamBody) -> Option<Bytes> {
    match body {
        UpstreamBody::Bytes(b) => Some(b.clone()),
        UpstreamBody::Stream(_) => None,
    }
}

fn needs_internal_unwrap(provider: &str, proto: Proto, op: Op) -> bool {
    proto == Proto::Gemini
        && op == Op::GenerateContent
        && (provider.eq_ignore_ascii_case("geminicli")
            || provider.eq_ignore_ascii_case("antigravity"))
}

fn needs_internal_stream_unwrap(provider: &str, proto: Proto) -> bool {
    proto == Proto::Gemini
        && (provider.eq_ignore_ascii_case("geminicli")
            || provider.eq_ignore_ascii_case("antigravity"))
}

fn should_passthrough_native_gemini_stream(
    req_native: &Request,
    upstream_headers: &Headers,
) -> bool {
    if downstream_requests_gemini_sse(req_native) {
        return true;
    }
    !upstream_stream_is_sse(upstream_headers)
}

fn downstream_requests_gemini_sse(req_native: &Request) -> bool {
    let query = match req_native {
        Request::GenerateContent(GenerateContentRequest::GeminiStream(req)) => req.query.as_deref(),
        _ => None,
    };
    let Some(query) = query else {
        return false;
    };
    query_alt_value_is_sse(query)
}

fn query_alt_value_is_sse(query: &str) -> bool {
    query
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
        .any(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            key.eq_ignore_ascii_case("alt") && value.eq_ignore_ascii_case("sse")
        })
}

fn upstream_stream_is_sse(headers: &Headers) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

fn unwrap_internal_json_bytes(provider: &str, body: &Bytes) -> Result<Bytes, String> {
    let value: JsonValue =
        serde_json::from_slice(body).map_err(|err| format!("json_decode_failed: {err}"))?;
    let mut value = unwrap_internal_value(value);
    if provider.eq_ignore_ascii_case("antigravity") {
        normalize_gemini_parts(&mut value);
    }
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .map_err(|err| format!("json_encode_failed: {err}"))
}

fn map_internal_gemini_stream(provider: &str, mut rx_in: ByteStream) -> ByteStream {
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(32);
    let provider = provider.to_string();
    tokio::spawn(async move {
        let mut parser = SseParser::new();
        let mut pending: VecDeque<Bytes> = VecDeque::new();
        loop {
            if let Some(item) = pending.pop_front() {
                if tx.send(item).await.is_err() {
                    break;
                }
                continue;
            }
            match rx_in.recv().await {
                Some(chunk) => {
                    for ev in parser.push_bytes(&chunk) {
                        for mapped in map_internal_event_data(&provider, &ev.data) {
                            pending.push_back(mapped);
                        }
                    }
                }
                None => {
                    for ev in parser.finish() {
                        for mapped in map_internal_event_data(&provider, &ev.data) {
                            pending.push_back(mapped);
                        }
                    }
                    while let Some(item) = pending.pop_front() {
                        if tx.send(item).await.is_err() {
                            return;
                        }
                    }
                    break;
                }
            }
        }
    });
    rx
}

fn map_internal_event_data(provider: &str, data: &str) -> Vec<Bytes> {
    if data == "[DONE]" {
        return vec![Bytes::from_static(b"data: [DONE]\n\n")];
    }
    let value: JsonValue = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(_) => {
            let mut raw = Vec::with_capacity(data.len() + 8);
            raw.extend_from_slice(b"data: ");
            raw.extend_from_slice(data.as_bytes());
            raw.extend_from_slice(b"\n\n");
            return vec![Bytes::from(raw)];
        }
    };
    let mut out = Vec::new();
    let mut value = unwrap_internal_value(value);
    if provider.eq_ignore_ascii_case("antigravity") {
        normalize_gemini_parts(&mut value);
    }
    match value {
        JsonValue::Array(items) => {
            for item in items {
                let mut item = unwrap_internal_value(item);
                if provider.eq_ignore_ascii_case("antigravity") {
                    normalize_gemini_parts(&mut item);
                }
                if let Some(bytes) = sse_json_bytes(&item) {
                    out.push(bytes);
                }
            }
        }
        other => {
            if let Some(bytes) = sse_json_bytes(&other) {
                out.push(bytes);
            }
        }
    }
    out
}

fn unwrap_internal_value(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(mut map) => match map.remove("response") {
            Some(JsonValue::Object(mut inner)) => match inner.remove("response") {
                Some(nested) => nested,
                None => JsonValue::Object(inner),
            },
            Some(inner) => inner,
            None => JsonValue::Object(map),
        },
        other => other,
    }
}

fn normalize_gemini_parts(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            if let Some(JsonValue::Array(candidates)) = map.get_mut("candidates") {
                for candidate in candidates {
                    if let JsonValue::Object(candidate) = candidate
                        && let Some(JsonValue::Object(content)) = candidate.get_mut("content")
                    {
                        content
                            .entry("parts")
                            .or_insert_with(|| JsonValue::Array(Vec::new()));
                    }
                }
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                normalize_gemini_parts(item);
            }
        }
        _ => {}
    }
}

fn sse_json_bytes(value: &JsonValue) -> Option<Bytes> {
    let payload = serde_json::to_vec(value).ok()?;
    let mut data = Vec::with_capacity(payload.len() + 8);
    data.extend_from_slice(b"data: ");
    data.extend_from_slice(&payload);
    data.extend_from_slice(b"\n\n");
    Some(Bytes::from(data))
}

fn is_generate_op(op: Op) -> bool {
    matches!(op, Op::GenerateContent | Op::StreamGenerateContent)
}

fn extract_model_from_request(req: &Request) -> Option<String> {
    match req {
        Request::GenerateContent(inner) => match inner {
            GenerateContentRequest::Claude(req) => Some(claude_model_to_string(&req.body.model)),
            GenerateContentRequest::OpenAIChat(req) => Some(req.body.model.clone()),
            GenerateContentRequest::OpenAIResponse(req) => Some(req.body.model.clone()),
            GenerateContentRequest::Gemini(req) => Some(req.path.model.clone()),
            GenerateContentRequest::GeminiStream(req) => Some(req.path.model.clone()),
        },
        _ => None,
    }
}

fn claude_model_to_string(model: &ClaudeModel) -> String {
    match model {
        ClaudeModel::Custom(s) => s.clone(),
        ClaudeModel::Known(k) => serde_json::to_string(k)
            .unwrap_or_else(|_| format!("{k:?}"))
            .trim_matches('"')
            .to_string(),
    }
}

fn maybe_prefix_model_in_response(
    mut resp: Response,
    response_model_prefix_provider: Option<&str>,
) -> Response {
    let Some(provider) = response_model_prefix_provider else {
        return resp;
    };

    match &mut resp {
        Response::ModelList(r) => match r {
            ModelListResponse::Claude(v) => {
                for item in &mut v.data {
                    item.id = prefix_model_string(&item.id, provider);
                }
            }
            ModelListResponse::OpenAI(v) => {
                for item in &mut v.data {
                    item.id = prefix_model_string(&item.id, provider);
                }
            }
            ModelListResponse::Gemini(v) => {
                for item in &mut v.models {
                    item.name = prefix_gemini_model_name(&item.name, provider);
                }
            }
        },
        Response::ModelGet(r) => match r {
            ModelGetResponse::Claude(v) => {
                v.id = prefix_model_string(&v.id, provider);
            }
            ModelGetResponse::OpenAI(v) => {
                v.id = prefix_model_string(&v.id, provider);
            }
            ModelGetResponse::Gemini(v) => {
                v.name = prefix_gemini_model_name(&v.name, provider);
            }
        },
        Response::GenerateContent(r) => match r {
            GenerateContentResponse::Claude(v) => {
                v.model = maybe_prefix_claude_model(v.model.clone(), provider);
            }
            GenerateContentResponse::OpenAIChat(v) => {
                v.model = prefix_model_string(&v.model, provider);
            }
            GenerateContentResponse::OpenAIResponse(v) => {
                v.model = prefix_model_string(&v.model, provider);
            }
            GenerateContentResponse::Gemini(_) => {}
        },
        Response::CountTokens(_)
        | Response::ResponseGet(_)
        | Response::ResponseDelete(_)
        | Response::ResponseCancel(_)
        | Response::ResponseListInputItems(_)
        | Response::ResponseCompact(_)
        | Response::MemoryTraceSummarize(_) => {}
    }

    resp
}

fn maybe_prefix_model_in_stream_event(
    mut ev: StreamEvent,
    response_model_prefix_provider: Option<&str>,
) -> StreamEvent {
    let Some(provider) = response_model_prefix_provider else {
        return ev;
    };

    match &mut ev {
        StreamEvent::Claude(v) => {
            *v = maybe_prefix_claude_stream_event(v.clone(), provider);
        }
        StreamEvent::OpenAIChat(v) => {
            v.model = prefix_model_string(&v.model, provider);
        }
        StreamEvent::OpenAIResponse(v) => {
            *v = maybe_prefix_openai_response_stream_event(v.clone(), provider);
        }
        StreamEvent::Gemini(_) => {}
    }

    ev
}

fn maybe_prefix_claude_stream_event(
    mut ev: gproxy_protocol::claude::create_message::stream::BetaStreamEvent,
    provider: &str,
) -> gproxy_protocol::claude::create_message::stream::BetaStreamEvent {
    if let gproxy_protocol::claude::create_message::stream::BetaStreamEvent::Known(
        gproxy_protocol::claude::create_message::stream::BetaStreamEventKnown::MessageStart {
            message,
        },
    ) = &mut ev
    {
        message.model = maybe_prefix_claude_model(message.model.clone(), provider);
    }
    ev
}

fn maybe_prefix_openai_response_stream_event(
    ev: gproxy_protocol::openai::create_response::stream::ResponseStreamEvent,
    provider: &str,
) -> gproxy_protocol::openai::create_response::stream::ResponseStreamEvent {
    let mut value = match serde_json::to_value(&ev) {
        Ok(v) => v,
        Err(_) => return ev,
    };
    if let JsonValue::Object(map) = &mut value
        && let Some(JsonValue::Object(response)) = map.get_mut("response")
        && let Some(JsonValue::String(model)) = response.get_mut("model")
    {
        *model = prefix_model_string(model, provider);
    }

    serde_json::from_value(value).unwrap_or(ev)
}

fn maybe_prefix_claude_model(model: ClaudeModel, provider: &str) -> ClaudeModel {
    let model_name = claude_model_to_string(&model);
    if model_name.is_empty() {
        return model;
    }
    ClaudeModel::Custom(prefix_model_string(&model_name, provider))
}

fn prefix_model_string(model: &str, provider: &str) -> String {
    if model.is_empty() {
        return model.to_string();
    }
    if model == provider {
        return model.to_string();
    }
    let prefixed = format!("{provider}/");
    if model.starts_with(&prefixed) {
        return model.to_string();
    }
    format!("{provider}/{model}")
}

fn prefix_gemini_model_name(model: &str, provider: &str) -> String {
    let raw = model.strip_prefix("models/").unwrap_or(model);
    format!("models/{}", prefix_model_string(raw, provider))
}

fn json_error(status: u16, code: &str) -> UpstreamHttpResponse {
    json_error_with(status, code, serde_json::Value::Null)
}

fn json_error_with(
    status: u16,
    code: &str,
    detail: impl Into<serde_json::Value>,
) -> UpstreamHttpResponse {
    let mut headers: Headers = Vec::new();
    header_set(&mut headers, "content-type", "application/json");
    let body = serde_json::json!({
        "error": code,
        "detail": detail.into(),
    });
    let bytes = Bytes::from(serde_json::to_vec(&body).unwrap_or_default());
    UpstreamHttpResponse {
        status,
        headers,
        body: UpstreamBody::Bytes(bytes),
    }
}

fn error_response_from_provider_err(err: &ProviderError) -> UpstreamHttpResponse {
    match err {
        ProviderError::Unsupported(_) => json_error(501, "provider_unsupported"),
        ProviderError::InvalidConfig(_) => {
            json_error_with(500, "provider_invalid_config", format!("{err:?}"))
        }
        _ => json_error_with(500, "provider_error", format!("{err:?}")),
    }
}

fn failure_to_http(failure: UpstreamFailure) -> UpstreamHttpResponse {
    match failure {
        UpstreamFailure::Transport { kind: _, message } => {
            json_error_with(502, "upstream_transport_error", message)
        }
        UpstreamFailure::Http {
            status,
            headers,
            body,
        } => normalize_upstream_http_failure(status, headers, body),
    }
}

fn normalize_upstream_http_failure(
    status: u16,
    headers: Headers,
    body: Bytes,
) -> UpstreamHttpResponse {
    // Preserve native upstream JSON errors as-is.
    if upstream_http_error_is_json(&headers, &body) {
        return UpstreamHttpResponse {
            status,
            headers,
            body: UpstreamBody::Bytes(body),
        };
    }

    // Normalize non-JSON upstream error pages (for example Cloudflare HTML)
    // to a stable machine-readable payload for downstream clients.
    let detail = upstream_http_error_detail(&body);
    json_error_with(
        status,
        "upstream_http_error",
        serde_json::json!({
            "status": status,
            "detail": detail,
        }),
    )
}

fn upstream_http_error_is_json(headers: &Headers, body: &Bytes) -> bool {
    let content_type_is_json = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| {
            let value = value.to_ascii_lowercase();
            value.contains("application/json") || value.contains("+json")
        })
        .unwrap_or(false);

    if content_type_is_json {
        return true;
    }

    serde_json::from_slice::<serde_json::Value>(body).is_ok()
}

fn upstream_http_error_detail(body: &Bytes) -> String {
    let text = String::from_utf8_lossy(body);
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "upstream returned non-json error response".to_string();
    }
    const MAX_LEN: usize = 512;
    let mut out = compact.chars().take(MAX_LEN).collect::<String>();
    if compact.chars().count() > MAX_LEN {
        out.push_str("...");
    }
    out
}

fn failure_message(failure: &UpstreamFailure) -> String {
    match failure {
        UpstreamFailure::Transport { message, .. } => message.clone(),
        UpstreamFailure::Http { status, .. } => format!("http_status_{status}"),
    }
}

fn internal_request(path: String) -> UpstreamHttpRequest {
    UpstreamHttpRequest {
        method: HttpMethod::Get,
        url: path,
        headers: Vec::new(),
        body: None,
        is_stream: false,
    }
}

fn transport_kind_from_failure(
    failure: &UpstreamFailure,
) -> Option<gproxy_provider_core::provider::UpstreamTransportErrorKind> {
    match failure {
        UpstreamFailure::Transport { kind, .. } => Some(*kind),
        _ => None,
    }
}

fn is_auth_failure(failure: &UpstreamFailure) -> bool {
    matches!(
        failure,
        UpstreamFailure::Http { status, .. } if *status == 401 || *status == 403
    )
}

fn is_retryable_failure(failure: &UpstreamFailure) -> bool {
    match failure {
        UpstreamFailure::Transport { kind, .. } => matches!(
            kind,
            gproxy_provider_core::provider::UpstreamTransportErrorKind::Timeout
                | gproxy_provider_core::provider::UpstreamTransportErrorKind::ReadTimeout
                | gproxy_provider_core::provider::UpstreamTransportErrorKind::Connect
                | gproxy_provider_core::provider::UpstreamTransportErrorKind::Dns
                | gproxy_provider_core::provider::UpstreamTransportErrorKind::Tls
        ),
        UpstreamFailure::Http { status, .. } => {
            *status == 429 || (500..600).contains(status) || *status == 401 || *status == 403
        }
    }
}

fn retry_backoff_delay(attempt_no: u32) -> Duration {
    let step = attempt_no.saturating_sub(1).min(6);
    let base_ms = 200u64;
    let backoff = base_ms.saturating_mul(1u64 << step);
    let jitter = rand::random::<u64>() % (base_ms + 1);
    Duration::from_millis((backoff + jitter).min(2_000))
}

async fn backoff_sleep(attempt_no: u32) {
    let delay = retry_backoff_delay(attempt_no);
    if delay.as_millis() > 0 {
        tokio::time::sleep(delay).await;
    }
}
