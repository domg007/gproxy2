use gproxy_middleware::{OperationFamily as Op, ProtocolKind as Proto};

use crate::dispatch::{ProviderDispatchTable, TypedRoute, table_from_typed_routes};

const ROUTES: &[TypedRoute] = &[
    // Model list / get / count token
    (Op::ModelList, Proto::Claude, Op::ModelList, Proto::Claude),
    (Op::ModelList, Proto::OpenAi, Op::ModelList, Proto::OpenAi),
    (Op::ModelList, Proto::Gemini, Op::ModelList, Proto::Claude),
    (Op::ModelGet, Proto::Claude, Op::ModelGet, Proto::Claude),
    (Op::ModelGet, Proto::OpenAi, Op::ModelGet, Proto::OpenAi),
    (Op::ModelGet, Proto::Gemini, Op::ModelGet, Proto::Claude),
    (Op::CountToken, Proto::Claude, Op::CountToken, Proto::Claude),
    (Op::CountToken, Proto::OpenAi, Op::CountToken, Proto::Claude),
    (Op::CountToken, Proto::Gemini, Op::CountToken, Proto::Claude),
    // Generate content (non-stream)
    (
        Op::GenerateContent,
        Proto::Claude,
        Op::GenerateContent,
        Proto::Claude,
    ),
    (
        Op::GenerateContent,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::Claude,
    ),
    (
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::Gemini,
        Op::GenerateContent,
        Proto::Claude,
    ),
    // Generate content (stream)
    (
        Op::StreamGenerateContent,
        Proto::Claude,
        Op::StreamGenerateContent,
        Proto::Claude,
    ),
    (
        Op::StreamGenerateContent,
        Proto::OpenAi,
        Op::StreamGenerateContent,
        Proto::Claude,
    ),
    (
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::Gemini,
        Op::StreamGenerateContent,
        Proto::Claude,
    ),
    (
        Op::StreamGenerateContent,
        Proto::GeminiNDJson,
        Op::StreamGenerateContent,
        Proto::Claude,
    ),
    // Internal OpenAI ops routed into Claude generate
    (
        Op::Compact,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::Claude,
    ),
];

pub fn default_dispatch_table() -> ProviderDispatchTable {
    table_from_typed_routes(ROUTES)
}
