use crate::channel::BuiltinChannel;
use serde::{Deserialize, Serialize};

use super::{
    aistudio, anthropic, antigravity, cache_control::CacheBreakpointRule, claudecode, codex,
    custom, deepseek, geminicli, grok, groq, nvidia, openai, vertex, vertexexpress,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinChannelSettings {
    OpenAi(openai::OpenAiSettings),
    Grok(grok::GrokSettings),
    Anthropic(anthropic::AnthropicSettings),
    AiStudio(aistudio::AiStudioSettings),
    VertexExpress(vertexexpress::VertexExpressSettings),
    Vertex(vertex::VertexSettings),
    GeminiCli(geminicli::GeminiCliSettings),
    ClaudeCode(claudecode::ClaudeCodeSettings),
    Codex(codex::CodexSettings),
    Antigravity(antigravity::AntigravitySettings),
    Nvidia(nvidia::NvidiaSettings),
    Deepseek(deepseek::DeepseekSettings),
    Groq(groq::GroqSettings),
}

impl BuiltinChannelSettings {
    pub fn default_for(channel: BuiltinChannel) -> Self {
        match channel {
            BuiltinChannel::OpenAi => Self::OpenAi(Default::default()),
            BuiltinChannel::Grok => Self::Grok(Default::default()),
            BuiltinChannel::Anthropic => Self::Anthropic(Default::default()),
            BuiltinChannel::AiStudio => Self::AiStudio(Default::default()),
            BuiltinChannel::VertexExpress => Self::VertexExpress(Default::default()),
            BuiltinChannel::Vertex => Self::Vertex(Default::default()),
            BuiltinChannel::GeminiCli => Self::GeminiCli(Default::default()),
            BuiltinChannel::ClaudeCode => Self::ClaudeCode(Default::default()),
            BuiltinChannel::Codex => Self::Codex(Default::default()),
            BuiltinChannel::Antigravity => Self::Antigravity(Default::default()),
            BuiltinChannel::Nvidia => Self::Nvidia(Default::default()),
            BuiltinChannel::Deepseek => Self::Deepseek(Default::default()),
            BuiltinChannel::Groq => Self::Groq(Default::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelSettings {
    Builtin(BuiltinChannelSettings),
    Custom(custom::CustomChannelSettings),
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self::Custom(custom::CustomChannelSettings::default())
    }
}

impl ChannelSettings {
    pub fn base_url(&self) -> &str {
        match self {
            Self::Builtin(BuiltinChannelSettings::OpenAi(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Grok(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::AiStudio(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::VertexExpress(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Vertex(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::GeminiCli(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Codex(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Antigravity(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Nvidia(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Deepseek(value)) => value.base_url.as_str(),
            Self::Builtin(BuiltinChannelSettings::Groq(value)) => value.base_url.as_str(),
            Self::Custom(value) => value.base_url.as_str(),
        }
    }

    pub fn user_agent(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::OpenAi(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Grok(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::AiStudio(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::VertexExpress(value)) => {
                value.user_agent.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Vertex(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::GeminiCli(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Codex(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Antigravity(value)) => {
                value.user_agent.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Nvidia(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Deepseek(value)) => value.user_agent.as_deref(),
            Self::Builtin(BuiltinChannelSettings::Groq(value)) => value.user_agent.as_deref(),
            Self::Custom(value) => value.user_agent.as_deref(),
        }
    }

    pub fn oauth_issuer_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::Codex(value)) => {
                value.oauth_issuer_url.as_deref()
            }
            _ => None,
        }
    }

    pub fn oauth_authorize_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::GeminiCli(value)) => {
                value.oauth_authorize_url.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Antigravity(value)) => {
                value.oauth_authorize_url.as_deref()
            }
            _ => None,
        }
    }

    pub fn oauth_token_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::GeminiCli(value)) => {
                value.oauth_token_url.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Antigravity(value)) => {
                value.oauth_token_url.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Vertex(value)) => {
                Some(value.oauth_token_url.as_str())
            }
            _ => None,
        }
    }

    pub fn oauth_userinfo_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::GeminiCli(value)) => {
                value.oauth_userinfo_url.as_deref()
            }
            Self::Builtin(BuiltinChannelSettings::Antigravity(value)) => {
                value.oauth_userinfo_url.as_deref()
            }
            _ => None,
        }
    }

    pub fn claudecode_ai_base_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => {
                Some(value.claude_ai_base_url.as_str())
            }
            _ => None,
        }
    }

    pub fn claudecode_platform_base_url(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => {
                Some(value.platform_base_url.as_str())
            }
            _ => None,
        }
    }

    pub fn anthropic_prelude_text(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => {
                value.prelude_text.as_deref()
            }
            _ => None,
        }
    }

    pub fn anthropic_extra_beta_headers(&self) -> &[String] {
        match self {
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => &value.extra_beta_headers,
            _ => &[],
        }
    }

    pub fn anthropic_append_beta_query(&self) -> bool {
        match self {
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => value.append_beta_query,
            _ => false,
        }
    }

    pub fn claudecode_prelude_text(&self) -> Option<&str> {
        match self {
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => {
                value.prelude_text.as_deref()
            }
            _ => None,
        }
    }

    pub fn claudecode_extra_beta_headers(&self) -> &[String] {
        match self {
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => &value.extra_beta_headers,
            _ => &[],
        }
    }

    pub fn claudecode_append_beta_query(&self) -> bool {
        match self {
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => value.append_beta_query,
            _ => false,
        }
    }

    pub fn cache_breakpoints(&self) -> &[CacheBreakpointRule] {
        match self {
            Self::Builtin(BuiltinChannelSettings::Anthropic(value)) => &value.cache_breakpoints,
            Self::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => &value.cache_breakpoints,
            _ => &[],
        }
    }

    pub fn custom_mask_table(&self) -> Option<&custom::settings::CustomMaskTable> {
        match self {
            Self::Custom(value) => Some(&value.mask_table),
            _ => None,
        }
    }
}
