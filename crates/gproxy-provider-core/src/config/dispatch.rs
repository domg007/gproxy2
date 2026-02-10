use serde::{Deserialize, Serialize};

use crate::{Op, Proto, TransformContext};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    // Claude
    ClaudeGenerate = 0,
    ClaudeGenerateStream = 1,
    ClaudeCountTokens = 2,
    ClaudeModelsList = 3,
    ClaudeModelsGet = 4,
    // Gemini
    GeminiGenerate = 5,
    GeminiGenerateStream = 6,
    GeminiCountTokens = 7,
    GeminiModelsList = 8,
    GeminiModelsGet = 9,
    // OpenAI chat completions
    OpenAIChatGenerate = 10,
    OpenAIChatGenerateStream = 11,
    // OpenAI Responses
    OpenAIResponseGenerate = 12,
    OpenAIResponseGenerateStream = 13,
    // OpenAI basic ops
    OpenAIInputTokens = 14,
    OpenAIModelsList = 15,
    OpenAIModelsGet = 16,
    // Extra internal ops (not covered by `TransformContext`)
    OAuthStart = 17,
    OAuthCallback = 18,
    Usage = 19,
}

impl OperationKind {
    pub const COUNT: usize = 20;

    pub fn from_context(ctx: &TransformContext) -> Option<Self> {
        match ctx.src_op {
            Op::GenerateContent => match ctx.src {
                Proto::Claude => Some(OperationKind::ClaudeGenerate),
                Proto::Gemini => Some(OperationKind::GeminiGenerate),
                Proto::OpenAIChat => Some(OperationKind::OpenAIChatGenerate),
                Proto::OpenAIResponse => Some(OperationKind::OpenAIResponseGenerate),
                Proto::OpenAI => None,
            },
            Op::StreamGenerateContent => match ctx.src {
                Proto::Claude => Some(OperationKind::ClaudeGenerateStream),
                Proto::Gemini => Some(OperationKind::GeminiGenerateStream),
                Proto::OpenAIChat => Some(OperationKind::OpenAIChatGenerateStream),
                Proto::OpenAIResponse => Some(OperationKind::OpenAIResponseGenerateStream),
                Proto::OpenAI => None,
            },
            Op::CountTokens => match ctx.src {
                Proto::Claude => Some(OperationKind::ClaudeCountTokens),
                Proto::Gemini => Some(OperationKind::GeminiCountTokens),
                Proto::OpenAI => Some(OperationKind::OpenAIInputTokens),
                _ => None,
            },
            Op::ModelList => match ctx.src {
                Proto::Claude => Some(OperationKind::ClaudeModelsList),
                Proto::Gemini => Some(OperationKind::GeminiModelsList),
                Proto::OpenAI => Some(OperationKind::OpenAIModelsList),
                _ => None,
            },
            Op::ModelGet => match ctx.src {
                Proto::Claude => Some(OperationKind::ClaudeModelsGet),
                Proto::Gemini => Some(OperationKind::GeminiModelsGet),
                Proto::OpenAI => Some(OperationKind::OpenAIModelsGet),
                _ => None,
            },
            Op::ResponseGet
            | Op::ResponseDelete
            | Op::ResponseCancel
            | Op::ResponseListInputItems
            | Op::ResponseCompact
            | Op::MemoryTraceSummarize => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchRule {
    /// The provider can handle this request in its current protocol/shape (no transform needed).
    Native,
    /// Transform to the target protocol first, then call the provider in that protocol.
    Transform { target: Proto },
    /// Not supported by this provider.
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DispatchTable {
    ops: [DispatchRule; OperationKind::COUNT],
}

impl DispatchTable {
    pub const fn new(ops: [DispatchRule; OperationKind::COUNT]) -> Self {
        Self { ops }
    }

    pub fn rule(&self, kind: OperationKind) -> DispatchRule {
        self.ops[kind as usize]
    }
    pub fn rule_for_context(&self, ctx: &TransformContext) -> DispatchRule {
        match OperationKind::from_context(ctx) {
            Some(kind) => self.rule(kind),
            None => DispatchRule::Unsupported,
        }
    }
}

impl Default for DispatchTable {
    fn default() -> Self {
        Self {
            ops: [DispatchRule::Unsupported; OperationKind::COUNT],
        }
    }
}
