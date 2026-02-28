use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuiltinChannel {
    OpenAi,
    Claude,
    AiStudio,
    VertexExpress,
    Vertex,
    GeminiCli,
    ClaudeCode,
    Codex,
    Antigravity,
    Nvidia,
    Deepseek,
}

impl BuiltinChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Claude => "claude",
            Self::AiStudio => "aistudio",
            Self::VertexExpress => "vertexexpress",
            Self::Vertex => "vertex",
            Self::GeminiCli => "geminicli",
            Self::ClaudeCode => "claudecode",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Nvidia => "nvidia",
            Self::Deepseek => "deepseek",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai" => Some(Self::OpenAi),
            "claude" => Some(Self::Claude),
            "aistudio" => Some(Self::AiStudio),
            "vertexexpress" => Some(Self::VertexExpress),
            "vertex" => Some(Self::Vertex),
            "geminicli" => Some(Self::GeminiCli),
            "claudecode" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "antigravity" => Some(Self::Antigravity),
            "nvidia" => Some(Self::Nvidia),
            "deepseek" => Some(Self::Deepseek),
            _ => None,
        }
    }
}

pub const BUILTIN_CHANNELS: [BuiltinChannel; 11] = [
    BuiltinChannel::OpenAi,
    BuiltinChannel::Claude,
    BuiltinChannel::AiStudio,
    BuiltinChannel::VertexExpress,
    BuiltinChannel::Vertex,
    BuiltinChannel::GeminiCli,
    BuiltinChannel::ClaudeCode,
    BuiltinChannel::Codex,
    BuiltinChannel::Antigravity,
    BuiltinChannel::Nvidia,
    BuiltinChannel::Deepseek,
];

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
