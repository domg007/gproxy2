use crate::channel::BuiltinChannel;
use serde::{Deserialize, Serialize};

use super::{
    aistudio, anthropic, antigravity, claudecode, codex, custom, deepseek, geminicli, grok, groq,
    nvidia, openai, vertex, vertexexpress,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinChannelCredential {
    OpenAi(openai::OpenAiCredential),
    Grok(grok::GrokCredential),
    Anthropic(anthropic::AnthropicCredential),
    AiStudio(aistudio::AiStudioCredential),
    VertexExpress(vertexexpress::VertexExpressCredential),
    Vertex(Box<vertex::VertexServiceAccountCredential>),
    GeminiCli(geminicli::GeminiCliCredential),
    ClaudeCode(claudecode::ClaudeCodeCredential),
    Codex(codex::CodexCredential),
    Antigravity(antigravity::AntigravityCredential),
    Nvidia(nvidia::NvidiaCredential),
    Deepseek(deepseek::DeepseekCredential),
    Groq(groq::GroqCredential),
}

impl BuiltinChannelCredential {
    pub fn blank_for(channel: BuiltinChannel) -> Self {
        match channel {
            BuiltinChannel::OpenAi => Self::OpenAi(Default::default()),
            BuiltinChannel::Grok => Self::Grok(Default::default()),
            BuiltinChannel::Anthropic => Self::Anthropic(Default::default()),
            BuiltinChannel::AiStudio => Self::AiStudio(Default::default()),
            BuiltinChannel::VertexExpress => Self::VertexExpress(Default::default()),
            BuiltinChannel::Vertex => {
                Self::Vertex(Box::<vertex::VertexServiceAccountCredential>::default())
            }
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
pub enum ChannelCredential {
    Builtin(BuiltinChannelCredential),
    Custom(custom::CustomChannelCredential),
}
