//! Shared operation taxonomy and endpoint metadata.

use serde::{Deserialize, Serialize};

/// Upstream protocol family.
///
/// Provider-specific wire modules (`openai`, `claude`, `gemini`) should reuse
/// this enum when declaring endpoint metadata or routing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAi,
    Claude,
    Gemini,
}

/// Coarse operation family, used to organize protocol support by capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationGroup {
    Models,
    CountTokens,
    GenerateContent,
    Images,
    Embeddings,
    Compact,
}

/// Provider-neutral operation name.
///
/// Variants are capability-oriented. A provider module should model only the
/// variants that the provider actually exposes; unsupported operations are not
/// represented by synthetic request/response types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    ListModels,
    GetModel,
    CountTokens,
    GenerateContent,
    StreamGenerateContent,
    CreateImage,
    EditImage,
    CreateEmbedding,
    CompactContent,
}

impl Operation {
    /// Return the operation group for this operation.
    pub const fn group(self) -> OperationGroup {
        match self {
            Self::ListModels | Self::GetModel => OperationGroup::Models,
            Self::CountTokens => OperationGroup::CountTokens,
            Self::GenerateContent | Self::StreamGenerateContent => OperationGroup::GenerateContent,
            Self::CreateImage | Self::EditImage => OperationGroup::Images,
            Self::CreateEmbedding => OperationGroup::Embeddings,
            Self::CompactContent => OperationGroup::Compact,
        }
    }
}

/// HTTP method for an upstream endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// Provider endpoint metadata used by routing and protocol modules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    pub provider: Provider,
    pub operation: Operation,
    pub method: HttpMethod,
    /// Provider-relative path template, e.g. `/v1/chat/completions`.
    pub path: String,
}

impl Endpoint {
    pub fn new(
        provider: Provider,
        operation: Operation,
        method: HttpMethod,
        path: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            operation,
            method,
            path: path.into(),
        }
    }

    pub const fn group(&self) -> OperationGroup {
        self.operation.group()
    }
}
