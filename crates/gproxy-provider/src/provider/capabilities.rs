use super::*;

#[derive(Clone, Copy)]
pub(super) struct ProviderChannelCapabilities {
    pub(super) execute: ExecuteHandler,
    pub(super) payload: Option<PayloadHandler>,
    pub(super) oauth_start: Option<OAuthStartHandler>,
    pub(super) oauth_callback: Option<OAuthCallbackHandler>,
    pub(super) upstream_usage: Option<UpstreamUsageHandler>,
}

macro_rules! define_execute_handler {
    ($name:ident, $handler:path, standard) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, spoof) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, tokenized) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
                context.token_resolution,
            ))
        }
    };
}

macro_rules! define_payload_handler {
    ($name:ident, $handler:path, split) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.payload.operation,
                context.payload.protocol,
                context.payload.body,
                context.payload.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, payload) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.payload,
            ))
        }
    };
    ($name:ident, $handler:path, spoof_payload) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.payload,
            ))
        }
    };
}

macro_rules! define_oauth_start_handler {
    ($name:ident, $handler:path) => {
        fn $name<'a>(context: OAuthContext<'a>) -> ProviderOAuthStartFuture<'a> {
            Box::pin($handler(
                context.client,
                &context.provider.settings,
                context.request,
            ))
        }
    };
}

macro_rules! define_oauth_callback_handler {
    ($name:ident, $handler:path) => {
        fn $name<'a>(context: OAuthContext<'a>) -> ProviderOAuthCallbackFuture<'a> {
            Box::pin($handler(
                context.client,
                &context.provider.settings,
                context.request,
            ))
        }
    };
}

macro_rules! define_upstream_usage_handler {
    ($name:ident, $handler:path, standard) => {
        fn $name<'a>(context: UpstreamUsageContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.credential_id,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, spoof) => {
        fn $name<'a>(context: UpstreamUsageContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.credential_id,
                context.now_unix_ms,
            ))
        }
    };
}

define_execute_handler!(
    execute_openai_capability,
    execute_openai_with_retry,
    standard
);
define_execute_handler!(
    execute_claude_capability,
    execute_claude_with_retry,
    standard
);
define_execute_handler!(
    execute_claudecode_capability,
    execute_claudecode_with_retry,
    spoof
);
define_execute_handler!(
    execute_aistudio_capability,
    execute_aistudio_with_retry,
    standard
);
define_execute_handler!(
    execute_vertexexpress_capability,
    execute_vertexexpress_with_retry,
    standard
);
define_execute_handler!(
    execute_vertex_capability,
    execute_vertex_with_retry,
    standard
);
define_execute_handler!(
    execute_geminicli_capability,
    execute_geminicli_with_retry,
    standard
);
define_execute_handler!(
    execute_codex_capability,
    execute_codex_with_retry,
    tokenized
);
define_execute_handler!(
    execute_antigravity_capability,
    execute_antigravity_with_retry,
    standard
);
define_execute_handler!(
    execute_nvidia_capability,
    execute_nvidia_with_retry,
    tokenized
);
define_execute_handler!(
    execute_deepseek_capability,
    execute_deepseek_with_retry,
    tokenized
);
define_execute_handler!(execute_groq_capability, execute_groq_with_retry, tokenized);
define_execute_handler!(
    execute_custom_capability,
    execute_custom_with_retry,
    standard
);

define_payload_handler!(
    payload_openai_capability,
    execute_openai_payload_with_retry,
    split
);
define_payload_handler!(
    payload_claude_capability,
    execute_claude_payload_with_retry,
    split
);
define_payload_handler!(
    payload_claudecode_capability,
    execute_claudecode_payload_with_retry,
    spoof_payload
);
define_payload_handler!(
    payload_aistudio_capability,
    execute_aistudio_payload_with_retry,
    split
);
define_payload_handler!(
    payload_vertexexpress_capability,
    execute_vertexexpress_payload_with_retry,
    split
);
define_payload_handler!(
    payload_vertex_capability,
    execute_vertex_payload_with_retry,
    split
);
define_payload_handler!(
    payload_geminicli_capability,
    execute_geminicli_payload_with_retry,
    split
);
define_payload_handler!(
    payload_codex_capability,
    execute_codex_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_antigravity_capability,
    execute_antigravity_payload_with_retry,
    split
);
define_payload_handler!(
    payload_nvidia_capability,
    execute_nvidia_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_deepseek_capability,
    execute_deepseek_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_groq_capability,
    execute_groq_payload_with_retry,
    payload
);

define_oauth_start_handler!(
    oauth_start_claudecode_capability,
    execute_claudecode_oauth_start
);
define_oauth_start_handler!(
    oauth_start_geminicli_capability,
    execute_geminicli_oauth_start
);
define_oauth_start_handler!(oauth_start_codex_capability, execute_codex_oauth_start);
define_oauth_start_handler!(
    oauth_start_antigravity_capability,
    execute_antigravity_oauth_start
);

define_oauth_callback_handler!(
    oauth_callback_claudecode_capability,
    execute_claudecode_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_geminicli_capability,
    execute_geminicli_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_codex_capability,
    execute_codex_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_antigravity_capability,
    execute_antigravity_oauth_callback
);

define_upstream_usage_handler!(
    usage_claudecode_capability,
    execute_claudecode_upstream_usage_with_retry,
    spoof
);
define_upstream_usage_handler!(
    usage_geminicli_capability,
    execute_geminicli_upstream_usage_with_retry,
    standard
);
define_upstream_usage_handler!(
    usage_codex_capability,
    execute_codex_upstream_usage_with_retry,
    standard
);
define_upstream_usage_handler!(
    usage_antigravity_capability,
    execute_antigravity_upstream_usage_with_retry,
    standard
);

pub(super) fn channel_capabilities(channel: &ChannelId) -> ProviderChannelCapabilities {
    match channel {
        ChannelId::Builtin(BuiltinChannel::OpenAi) => ProviderChannelCapabilities {
            execute: execute_openai_capability,
            payload: Some(payload_openai_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Claude) => ProviderChannelCapabilities {
            execute: execute_claude_capability,
            payload: Some(payload_claude_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::ClaudeCode) => ProviderChannelCapabilities {
            execute: execute_claudecode_capability,
            payload: Some(payload_claudecode_capability),
            oauth_start: Some(oauth_start_claudecode_capability),
            oauth_callback: Some(oauth_callback_claudecode_capability),
            upstream_usage: Some(usage_claudecode_capability),
        },
        ChannelId::Builtin(BuiltinChannel::AiStudio) => ProviderChannelCapabilities {
            execute: execute_aistudio_capability,
            payload: Some(payload_aistudio_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::VertexExpress) => ProviderChannelCapabilities {
            execute: execute_vertexexpress_capability,
            payload: Some(payload_vertexexpress_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Vertex) => ProviderChannelCapabilities {
            execute: execute_vertex_capability,
            payload: Some(payload_vertex_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => ProviderChannelCapabilities {
            execute: execute_geminicli_capability,
            payload: Some(payload_geminicli_capability),
            oauth_start: Some(oauth_start_geminicli_capability),
            oauth_callback: Some(oauth_callback_geminicli_capability),
            upstream_usage: Some(usage_geminicli_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Codex) => ProviderChannelCapabilities {
            execute: execute_codex_capability,
            payload: Some(payload_codex_capability),
            oauth_start: Some(oauth_start_codex_capability),
            oauth_callback: Some(oauth_callback_codex_capability),
            upstream_usage: Some(usage_codex_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Antigravity) => ProviderChannelCapabilities {
            execute: execute_antigravity_capability,
            payload: Some(payload_antigravity_capability),
            oauth_start: Some(oauth_start_antigravity_capability),
            oauth_callback: Some(oauth_callback_antigravity_capability),
            upstream_usage: Some(usage_antigravity_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Nvidia) => ProviderChannelCapabilities {
            execute: execute_nvidia_capability,
            payload: Some(payload_nvidia_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Deepseek) => ProviderChannelCapabilities {
            execute: execute_deepseek_capability,
            payload: Some(payload_deepseek_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Groq) => ProviderChannelCapabilities {
            execute: execute_groq_capability,
            payload: Some(payload_groq_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Custom(_) => ProviderChannelCapabilities {
            execute: execute_custom_capability,
            payload: None,
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderChannelCapabilities, channel_capabilities};
    use crate::channel::ChannelId;
    use crate::registry::BUILTIN_CHANNEL_REGISTRY;

    fn assert_builtin_payload_support(capabilities: ProviderChannelCapabilities, channel: &str) {
        assert!(
            capabilities.payload.is_some(),
            "builtin channel {channel} should support payload execution"
        );
    }

    #[test]
    fn builtin_capabilities_match_registry_flags() {
        for entry in BUILTIN_CHANNEL_REGISTRY {
            let capabilities = channel_capabilities(&ChannelId::Builtin(entry.channel));
            assert_eq!(
                capabilities.oauth_start.is_some() && capabilities.oauth_callback.is_some(),
                entry.supports_oauth,
                "channel {} oauth capability mismatch",
                entry.id
            );
            assert_eq!(
                capabilities.upstream_usage.is_some(),
                entry.supports_upstream_usage,
                "channel {} upstream usage capability mismatch",
                entry.id
            );
            assert_builtin_payload_support(capabilities, entry.id);
        }
    }

    #[test]
    fn custom_channel_capabilities_only_support_execute() {
        let capabilities = channel_capabilities(&ChannelId::custom("custom-provider"));
        assert!(capabilities.payload.is_none());
        assert!(capabilities.oauth_start.is_none());
        assert!(capabilities.oauth_callback.is_none());
        assert!(capabilities.upstream_usage.is_none());
    }
}
