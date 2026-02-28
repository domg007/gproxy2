use gproxy_middleware::{OperationFamily as Op, ProtocolKind as Proto};

use crate::dispatch::{ProviderDispatchTable, TypedRoute, table_from_typed_routes};

const ROUTES: &[TypedRoute] = &[
    // Model list / get / count token
    (Op::ModelList, Proto::OpenAi, Op::ModelList, Proto::OpenAi),
    (Op::ModelList, Proto::Claude, Op::ModelList, Proto::OpenAi),
    (Op::ModelList, Proto::Gemini, Op::ModelList, Proto::OpenAi),
    (Op::ModelGet, Proto::OpenAi, Op::ModelGet, Proto::OpenAi),
    (Op::ModelGet, Proto::Claude, Op::ModelGet, Proto::OpenAi),
    (Op::ModelGet, Proto::Gemini, Op::ModelGet, Proto::OpenAi),
    (Op::CountToken, Proto::OpenAi, Op::CountToken, Proto::OpenAi),
    (Op::CountToken, Proto::Claude, Op::CountToken, Proto::OpenAi),
    (Op::CountToken, Proto::Gemini, Op::CountToken, Proto::OpenAi),
    // Generate content (non-stream)
    (
        Op::GenerateContent,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::Claude,
        Op::GenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::GenerateContent,
        Proto::Gemini,
        Op::GenerateContent,
        Proto::OpenAi,
    ),
    // Generate content (stream)
    (
        Op::StreamGenerateContent,
        Proto::OpenAi,
        Op::StreamGenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::Claude,
        Op::StreamGenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::StreamGenerateContent,
        Proto::Gemini,
        Op::StreamGenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::StreamGenerateContent,
        Proto::GeminiNDJson,
        Op::StreamGenerateContent,
        Proto::OpenAi,
    ),
    // Embeddings
    (Op::Embedding, Proto::OpenAi, Op::Embedding, Proto::OpenAi),
    (Op::Embedding, Proto::Gemini, Op::Embedding, Proto::OpenAi),
    // OpenAI-only internal ops
    (Op::Compact, Proto::OpenAi, Op::Compact, Proto::OpenAi),
];

pub fn default_dispatch_table() -> ProviderDispatchTable {
    table_from_typed_routes(ROUTES)
}
