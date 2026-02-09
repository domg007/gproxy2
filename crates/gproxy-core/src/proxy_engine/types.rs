use gproxy_provider_core::{
    OAuthCallbackRequest, OAuthStartRequest, Op, OpenAIResponsesPassthroughRequest, Proto, Request,
};

#[derive(Debug, Clone)]
pub struct ProxyAuth {
    pub user_id: i64,
    pub user_key_id: i64,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ProxyCall {
    Protocol {
        trace_id: Option<String>,
        auth: ProxyAuth,
        provider: String,
        response_model_prefix_provider: Option<String>,
        user_proto: Proto,
        user_op: Op,
        req: Box<Request>,
    },
    OpenAIResponsesPassthrough {
        trace_id: Option<String>,
        auth: ProxyAuth,
        provider: String,
        req: OpenAIResponsesPassthroughRequest,
    },
    OAuthStart {
        trace_id: Option<String>,
        auth: ProxyAuth,
        provider: String,
        req: OAuthStartRequest,
    },
    OAuthCallback {
        trace_id: Option<String>,
        auth: ProxyAuth,
        provider: String,
        req: OAuthCallbackRequest,
    },
    UpstreamUsage {
        trace_id: Option<String>,
        auth: ProxyAuth,
        provider: String,
        credential_id: i64,
    },
}
