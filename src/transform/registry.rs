use crate::protocol::{ContentGenerationKind, Operation, OperationKey, OperationKind, Provider};

use super::TransformError;

/// Stable identifier for a supported transform implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformPair {
    OpenAiResponsesToOpenAiChat,
    OpenAiChatToOpenAiResponses,
    OpenAiResponsesToClaudeMessages,
    ClaudeMessagesToOpenAiResponses,
    OpenAiResponsesToGeminiGenerateContent,
    GeminiGenerateContentToOpenAiResponses,
    OpenAiChatToClaudeMessages,
    ClaudeMessagesToOpenAiChat,
    OpenAiChatToGeminiGenerateContent,
    GeminiGenerateContentToOpenAiChat,
    ClaudeMessagesToGeminiGenerateContent,
    GeminiGenerateContentToClaudeMessages,
    OpenAiToClaudeCountTokens,
    ClaudeToOpenAiCountTokens,
    OpenAiToGeminiCountTokens,
    GeminiToOpenAiCountTokens,
    ClaudeToGeminiCountTokens,
    GeminiToClaudeCountTokens,
    OpenAiToClaudeModels,
    ClaudeToOpenAiModels,
    OpenAiToGeminiModels,
    GeminiToOpenAiModels,
    ClaudeToGeminiModels,
    GeminiToClaudeModels,
    OpenAiToGeminiEmbeddings,
    GeminiToOpenAiEmbeddings,
    OpenAiToGeminiImages,
    GeminiToOpenAiImages,
    OpenAiToClaudeCompact,
    ClaudeToOpenAiCompact,
}

/// Resolve operation keys to a concrete pair module.
///
/// Same-kind passthrough is intentionally not represented here; routing should
/// bypass the transform layer when source and target wire kinds match.
pub fn resolve(
    source: OperationKey,
    target: OperationKey,
) -> Result<TransformPair, TransformError> {
    if !source.is_consistent() {
        return Err(TransformError::InvalidInput {
            reason: "source operation and kind are inconsistent".to_owned(),
        });
    }
    if !target.is_consistent() {
        return Err(TransformError::InvalidInput {
            reason: "target operation and kind are inconsistent".to_owned(),
        });
    }
    if source == target {
        return Err(TransformError::unsupported_pair(source, target));
    }
    if source.operation != target.operation {
        return Err(TransformError::unsupported_pair(source, target));
    }

    match source.operation {
        Operation::GenerateContent => resolve_content_generation(source, target),
        Operation::StreamGenerateContent => Err(TransformError::unsupported_pair(source, target)),
        Operation::CountTokens => resolve_provider_pair(source, target, count_tokens_pair),
        Operation::ListModels | Operation::GetModel => {
            resolve_provider_pair(source, target, models_pair)
        }
        Operation::CreateEmbedding => resolve_provider_pair(source, target, embeddings_pair),
        Operation::CreateImage | Operation::EditImage => {
            resolve_provider_pair(source, target, images_pair)
        }
        Operation::CompactContent => resolve_provider_pair(source, target, compact_pair),
        Operation::CreateConversation => Err(TransformError::unsupported_pair(source, target)),
    }
}

fn resolve_content_generation(
    source: OperationKey,
    target: OperationKey,
) -> Result<TransformPair, TransformError> {
    let OperationKind::ContentGeneration(source_kind) = source.kind else {
        return Err(TransformError::unsupported_pair(source, target));
    };
    let OperationKind::ContentGeneration(target_kind) = target.kind else {
        return Err(TransformError::unsupported_pair(source, target));
    };

    use ContentGenerationKind as Kind;
    use TransformPair as Pair;

    match (source_kind, target_kind) {
        (Kind::OpenAiResponses, Kind::OpenAiChatCompletions) => {
            Ok(Pair::OpenAiResponsesToOpenAiChat)
        }
        (Kind::OpenAiChatCompletions, Kind::OpenAiResponses) => {
            Ok(Pair::OpenAiChatToOpenAiResponses)
        }
        (Kind::OpenAiResponses, Kind::ClaudeMessages) => Ok(Pair::OpenAiResponsesToClaudeMessages),
        (Kind::ClaudeMessages, Kind::OpenAiResponses) => Ok(Pair::ClaudeMessagesToOpenAiResponses),
        (Kind::OpenAiResponses, Kind::GeminiGenerateContent) => {
            Ok(Pair::OpenAiResponsesToGeminiGenerateContent)
        }
        (Kind::GeminiGenerateContent, Kind::OpenAiResponses) => {
            Ok(Pair::GeminiGenerateContentToOpenAiResponses)
        }
        (Kind::OpenAiChatCompletions, Kind::ClaudeMessages) => Ok(Pair::OpenAiChatToClaudeMessages),
        (Kind::ClaudeMessages, Kind::OpenAiChatCompletions) => Ok(Pair::ClaudeMessagesToOpenAiChat),
        (Kind::OpenAiChatCompletions, Kind::GeminiGenerateContent) => {
            Ok(Pair::OpenAiChatToGeminiGenerateContent)
        }
        (Kind::GeminiGenerateContent, Kind::OpenAiChatCompletions) => {
            Ok(Pair::GeminiGenerateContentToOpenAiChat)
        }
        (Kind::ClaudeMessages, Kind::GeminiGenerateContent) => {
            Ok(Pair::ClaudeMessagesToGeminiGenerateContent)
        }
        (Kind::GeminiGenerateContent, Kind::ClaudeMessages) => {
            Ok(Pair::GeminiGenerateContentToClaudeMessages)
        }
        _ => Err(TransformError::unsupported_pair(source, target)),
    }
}

fn resolve_provider_pair(
    source: OperationKey,
    target: OperationKey,
    pair_fn: fn(Provider, Provider) -> Option<TransformPair>,
) -> Result<TransformPair, TransformError> {
    let OperationKind::Provider(source_provider) = source.kind else {
        return Err(TransformError::unsupported_pair(source, target));
    };
    let OperationKind::Provider(target_provider) = target.kind else {
        return Err(TransformError::unsupported_pair(source, target));
    };

    pair_fn(source_provider, target_provider)
        .ok_or_else(|| TransformError::unsupported_pair(source, target))
}

fn count_tokens_pair(source: Provider, target: Provider) -> Option<TransformPair> {
    provider_matrix(
        source,
        target,
        ProviderMatrix {
            openai_to_claude: TransformPair::OpenAiToClaudeCountTokens,
            claude_to_openai: TransformPair::ClaudeToOpenAiCountTokens,
            openai_to_gemini: TransformPair::OpenAiToGeminiCountTokens,
            gemini_to_openai: TransformPair::GeminiToOpenAiCountTokens,
            claude_to_gemini: TransformPair::ClaudeToGeminiCountTokens,
            gemini_to_claude: TransformPair::GeminiToClaudeCountTokens,
        },
    )
}

fn models_pair(source: Provider, target: Provider) -> Option<TransformPair> {
    provider_matrix(
        source,
        target,
        ProviderMatrix {
            openai_to_claude: TransformPair::OpenAiToClaudeModels,
            claude_to_openai: TransformPair::ClaudeToOpenAiModels,
            openai_to_gemini: TransformPair::OpenAiToGeminiModels,
            gemini_to_openai: TransformPair::GeminiToOpenAiModels,
            claude_to_gemini: TransformPair::ClaudeToGeminiModels,
            gemini_to_claude: TransformPair::GeminiToClaudeModels,
        },
    )
}

fn embeddings_pair(source: Provider, target: Provider) -> Option<TransformPair> {
    match (source, target) {
        (Provider::OpenAi, Provider::Gemini) => Some(TransformPair::OpenAiToGeminiEmbeddings),
        (Provider::Gemini, Provider::OpenAi) => Some(TransformPair::GeminiToOpenAiEmbeddings),
        _ => None,
    }
}

fn images_pair(source: Provider, target: Provider) -> Option<TransformPair> {
    match (source, target) {
        (Provider::OpenAi, Provider::Gemini) => Some(TransformPair::OpenAiToGeminiImages),
        (Provider::Gemini, Provider::OpenAi) => Some(TransformPair::GeminiToOpenAiImages),
        _ => None,
    }
}

fn compact_pair(source: Provider, target: Provider) -> Option<TransformPair> {
    match (source, target) {
        (Provider::OpenAi, Provider::Claude) => Some(TransformPair::OpenAiToClaudeCompact),
        (Provider::Claude, Provider::OpenAi) => Some(TransformPair::ClaudeToOpenAiCompact),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct ProviderMatrix {
    openai_to_claude: TransformPair,
    claude_to_openai: TransformPair,
    openai_to_gemini: TransformPair,
    gemini_to_openai: TransformPair,
    claude_to_gemini: TransformPair,
    gemini_to_claude: TransformPair,
}

fn provider_matrix(
    source: Provider,
    target: Provider,
    matrix: ProviderMatrix,
) -> Option<TransformPair> {
    match (source, target) {
        (Provider::OpenAi, Provider::Claude) => Some(matrix.openai_to_claude),
        (Provider::Claude, Provider::OpenAi) => Some(matrix.claude_to_openai),
        (Provider::OpenAi, Provider::Gemini) => Some(matrix.openai_to_gemini),
        (Provider::Gemini, Provider::OpenAi) => Some(matrix.gemini_to_openai),
        (Provider::Claude, Provider::Gemini) => Some(matrix.claude_to_gemini),
        (Provider::Gemini, Provider::Claude) => Some(matrix.gemini_to_claude),
        _ => None,
    }
}
