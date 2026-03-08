use serde::{Deserialize, Serialize};

pub use crate::registry::BUILTIN_CHANNELS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuiltinChannel {
    OpenAi,
    Grok,
    Anthropic,
    AiStudio,
    VertexExpress,
    Vertex,
    GeminiCli,
    ClaudeCode,
    Codex,
    Antigravity,
    Nvidia,
    Deepseek,
    Groq,
}

impl BuiltinChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Grok => "grok-web",
            Self::Anthropic => "anthropic",
            Self::AiStudio => "aistudio",
            Self::VertexExpress => "vertexexpress",
            Self::Vertex => "vertex",
            Self::GeminiCli => "geminicli",
            Self::ClaudeCode => "claudecode",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Nvidia => "nvidia",
            Self::Deepseek => "deepseek",
            Self::Groq => "groq",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        crate::registry::parse_builtin_channel(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChannelId {
    Builtin(BuiltinChannel),
    Custom(String),
}

impl ChannelId {
    pub const fn builtin(channel: BuiltinChannel) -> Self {
        Self::Builtin(channel)
    }

    pub fn custom(channel: impl Into<String>) -> Self {
        Self::Custom(channel.into())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Builtin(channel) => channel.as_str(),
            Self::Custom(channel) => channel.as_str(),
        }
    }

    pub fn parse(value: &str) -> Self {
        if let Some(channel) = BuiltinChannel::parse(value) {
            return Self::Builtin(channel);
        }
        Self::Custom(value.to_string())
    }
}

impl From<BuiltinChannel> for ChannelId {
    fn from(value: BuiltinChannel) -> Self {
        Self::Builtin(value)
    }
}
