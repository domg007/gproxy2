use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::{
    BuiltinChannelCredential, ChannelCredential, aistudio, anthropic, antigravity, claudecode,
    codex, deepseek, geminicli, grok, groq, nvidia, openai, vertex, vertexexpress,
};
use crate::dispatch::ProviderDispatchTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinChannelRegistration {
    pub channel: BuiltinChannel,
    pub id: &'static str,
    pub supports_oauth: bool,
    pub supports_upstream_usage: bool,
    pub supports_secret_credential: bool,
}

pub const BUILTIN_CHANNELS: [BuiltinChannel; 13] = [
    BuiltinChannel::OpenAi,
    BuiltinChannel::Grok,
    BuiltinChannel::Anthropic,
    BuiltinChannel::AiStudio,
    BuiltinChannel::VertexExpress,
    BuiltinChannel::Vertex,
    BuiltinChannel::GeminiCli,
    BuiltinChannel::ClaudeCode,
    BuiltinChannel::Codex,
    BuiltinChannel::Antigravity,
    BuiltinChannel::Nvidia,
    BuiltinChannel::Deepseek,
    BuiltinChannel::Groq,
];

pub const BUILTIN_CHANNEL_REGISTRY: [BuiltinChannelRegistration; 13] = [
    BuiltinChannelRegistration {
        channel: BuiltinChannel::OpenAi,
        id: "openai",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Grok,
        id: "grok-web",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Anthropic,
        id: "anthropic",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::AiStudio,
        id: "aistudio",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::VertexExpress,
        id: "vertexexpress",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Vertex,
        id: "vertex",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: false,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::GeminiCli,
        id: "geminicli",
        supports_oauth: true,
        supports_upstream_usage: true,
        supports_secret_credential: false,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::ClaudeCode,
        id: "claudecode",
        supports_oauth: true,
        supports_upstream_usage: true,
        supports_secret_credential: false,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Codex,
        id: "codex",
        supports_oauth: true,
        supports_upstream_usage: true,
        supports_secret_credential: false,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Antigravity,
        id: "antigravity",
        supports_oauth: true,
        supports_upstream_usage: true,
        supports_secret_credential: false,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Nvidia,
        id: "nvidia",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Deepseek,
        id: "deepseek",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
    BuiltinChannelRegistration {
        channel: BuiltinChannel::Groq,
        id: "groq",
        supports_oauth: false,
        supports_upstream_usage: false,
        supports_secret_credential: true,
    },
];

pub fn parse_builtin_channel(value: &str) -> Option<BuiltinChannel> {
    if value == "grok" {
        return Some(BuiltinChannel::Grok);
    }
    BUILTIN_CHANNEL_REGISTRY
        .iter()
        .find(|entry| entry.id == value)
        .map(|entry| entry.channel)
}

pub fn default_dispatch_table_for_builtin(channel: BuiltinChannel) -> ProviderDispatchTable {
    match channel {
        BuiltinChannel::OpenAi => openai::default_dispatch_table(),
        BuiltinChannel::Grok => grok::default_dispatch_table(),
        BuiltinChannel::Anthropic => anthropic::default_dispatch_table(),
        BuiltinChannel::AiStudio => aistudio::default_dispatch_table(),
        BuiltinChannel::VertexExpress => vertexexpress::default_dispatch_table(),
        BuiltinChannel::Vertex => vertex::default_dispatch_table(),
        BuiltinChannel::GeminiCli => geminicli::default_dispatch_table(),
        BuiltinChannel::ClaudeCode => claudecode::default_dispatch_table(),
        BuiltinChannel::Codex => codex::default_dispatch_table(),
        BuiltinChannel::Antigravity => antigravity::default_dispatch_table(),
        BuiltinChannel::Nvidia => nvidia::default_dispatch_table(),
        BuiltinChannel::Deepseek => deepseek::default_dispatch_table(),
        BuiltinChannel::Groq => groq::default_dispatch_table(),
    }
}

pub fn credential_from_secret(channel: &ChannelId, secret: &str) -> Option<ChannelCredential> {
    let secret = secret.trim();
    if secret.is_empty() {
        return None;
    }

    match channel {
        ChannelId::Custom(_) => Some(ChannelCredential::Custom(
            crate::channels::custom::CustomChannelCredential {
                api_key: secret.to_string(),
            },
        )),
        ChannelId::Builtin(builtin) => {
            let credential = match builtin {
                BuiltinChannel::OpenAi => ChannelCredential::Builtin(
                    BuiltinChannelCredential::OpenAi(openai::OpenAiCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::Grok => ChannelCredential::Builtin(BuiltinChannelCredential::Grok(
                    grok::GrokCredential {
                        sso: secret.to_string(),
                    },
                )),
                BuiltinChannel::Anthropic => ChannelCredential::Builtin(
                    BuiltinChannelCredential::Anthropic(anthropic::AnthropicCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::AiStudio => ChannelCredential::Builtin(
                    BuiltinChannelCredential::AiStudio(aistudio::AiStudioCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::VertexExpress => {
                    ChannelCredential::Builtin(BuiltinChannelCredential::VertexExpress(
                        vertexexpress::VertexExpressCredential {
                            api_key: secret.to_string(),
                        },
                    ))
                }
                BuiltinChannel::Nvidia => ChannelCredential::Builtin(
                    BuiltinChannelCredential::Nvidia(nvidia::NvidiaCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::Deepseek => ChannelCredential::Builtin(
                    BuiltinChannelCredential::Deepseek(deepseek::DeepseekCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::Groq => ChannelCredential::Builtin(BuiltinChannelCredential::Groq(
                    groq::GroqCredential {
                        api_key: secret.to_string(),
                    },
                )),
                BuiltinChannel::Vertex
                | BuiltinChannel::GeminiCli
                | BuiltinChannel::ClaudeCode
                | BuiltinChannel::Codex
                | BuiltinChannel::Antigravity => return None,
            };
            Some(credential)
        }
    }
}
