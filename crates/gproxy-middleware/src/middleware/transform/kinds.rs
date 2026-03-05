use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationFamily {
    ModelList,
    ModelGet,
    CountToken,
    Compact,
    GenerateContent,
    StreamGenerateContent,
    OpenAiResponseWebSocket,
    GeminiLive,
    Embedding,
}

impl OperationFamily {
    pub const fn is_stream(self) -> bool {
        matches!(self, Self::StreamGenerateContent)
    }

    pub const fn can_be_stream_driven(self) -> bool {
        matches!(self, Self::GenerateContent | Self::Compact)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolKind {
    OpenAi,
    Claude,
    Gemini,
    OpenAiChatCompletion,
    GeminiNDJson,
}

impl ProtocolKind {
    pub const fn normalize_gemini_stream(self) -> Self {
        match self {
            Self::GeminiNDJson => Self::Gemini,
            _ => self,
        }
    }
}
