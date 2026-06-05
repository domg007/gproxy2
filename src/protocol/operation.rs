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
    Conversation,
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
    CreateConversation,
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
            Self::CreateConversation => OperationGroup::Conversation,
        }
    }
}

/// Wire-format kind used together with [`Operation`].
///
/// Content generation needs a four-way kind because OpenAI has two distinct
/// native formats for the same capability. Non-content operations only need the
/// three provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OperationKind {
    ContentGeneration(ContentGenerationKind),
    Provider(Provider),
}

impl OperationKind {
    pub const fn provider(self) -> Provider {
        match self {
            Self::ContentGeneration(kind) => kind.provider(),
            Self::Provider(provider) => provider,
        }
    }

    pub const fn is_content_generation(self) -> bool {
        matches!(self, Self::ContentGeneration(_))
    }
}

/// Content-generation wire formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentGenerationKind {
    OpenAiResponses,
    OpenAiChatCompletions,
    ClaudeMessages,
    GeminiGenerateContent,
}

impl ContentGenerationKind {
    pub const fn provider(self) -> Provider {
        match self {
            Self::OpenAiResponses | Self::OpenAiChatCompletions => Provider::OpenAi,
            Self::ClaudeMessages => Provider::Claude,
            Self::GeminiGenerateContent => Provider::Gemini,
        }
    }
}

/// Capability plus wire-format kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationKey {
    pub operation: Operation,
    pub kind: OperationKind,
}

impl OperationKey {
    pub fn content_generation(operation: Operation, kind: ContentGenerationKind) -> Self {
        debug_assert!(
            operation.is_content_generation(),
            "content-generation kind used with non-content operation"
        );
        Self {
            operation,
            kind: OperationKind::ContentGeneration(kind),
        }
    }

    pub fn provider(operation: Operation, provider: Provider) -> Self {
        debug_assert!(
            !operation.is_content_generation(),
            "provider kind used with content-generation operation"
        );
        Self {
            operation,
            kind: OperationKind::Provider(provider),
        }
    }

    pub const fn group(self) -> OperationGroup {
        self.operation.group()
    }

    pub const fn provider_family(self) -> Provider {
        self.kind.provider()
    }

    pub const fn is_consistent(self) -> bool {
        self.operation.is_content_generation() == self.kind.is_content_generation()
    }
}

impl Operation {
    pub const fn is_content_generation(self) -> bool {
        matches!(self, Self::GenerateContent | Self::StreamGenerateContent)
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
    pub operation_key: OperationKey,
    pub method: HttpMethod,
    /// Provider-relative path template, e.g. `/v1/chat/completions`.
    pub path: String,
}

impl Endpoint {
    pub fn new(operation_key: OperationKey, method: HttpMethod, path: impl Into<String>) -> Self {
        Self {
            operation_key,
            method,
            path: path.into(),
        }
    }

    pub fn content_generation(
        operation: Operation,
        kind: ContentGenerationKind,
        method: HttpMethod,
        path: impl Into<String>,
    ) -> Self {
        Self::new(
            OperationKey::content_generation(operation, kind),
            method,
            path,
        )
    }

    pub fn provider(
        operation: Operation,
        provider: Provider,
        method: HttpMethod,
        path: impl Into<String>,
    ) -> Self {
        Self::new(OperationKey::provider(operation, provider), method, path)
    }

    pub const fn provider_family(&self) -> Provider {
        self.operation_key.provider_family()
    }

    pub const fn group(&self) -> OperationGroup {
        self.operation_key.group()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_content_generation_kinds_share_provider_family() {
        assert_eq!(
            ContentGenerationKind::OpenAiResponses.provider(),
            Provider::OpenAi
        );
        assert_eq!(
            ContentGenerationKind::OpenAiChatCompletions.provider(),
            Provider::OpenAi
        );
    }

    #[test]
    fn operation_key_distinguishes_content_generation_from_provider_kind() {
        let responses = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::OpenAiResponses,
        );
        let chat = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::OpenAiChatCompletions,
        );
        let models = OperationKey::provider(Operation::ListModels, Provider::OpenAi);

        assert_ne!(responses, chat);
        assert!(responses.is_consistent());
        assert!(chat.is_consistent());
        assert!(models.is_consistent());
        assert_eq!(responses.group(), OperationGroup::GenerateContent);
        assert_eq!(models.group(), OperationGroup::Models);
        assert_eq!(
            Operation::CreateConversation.group(),
            OperationGroup::Conversation
        );
    }

    #[test]
    fn operation_kind_serializes_as_flat_kind_value() {
        assert_eq!(
            serde_json::to_value(OperationKind::ContentGeneration(
                ContentGenerationKind::OpenAiResponses
            ))
            .expect("content-generation kind should serialize"),
            serde_json::json!("open_ai_responses")
        );
        assert_eq!(
            serde_json::to_value(OperationKind::Provider(Provider::OpenAi))
                .expect("provider kind should serialize"),
            serde_json::json!("open_ai")
        );
    }
}
