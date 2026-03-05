use gproxy_middleware::{OperationFamily as Op, ProtocolKind as Proto};

use crate::dispatch::{
    ProviderDispatchTable, RouteImplementation, RouteKey, TypedRoute, table_from_typed_routes,
};

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
        Proto::OpenAiChatCompletion,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::Claude,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::Gemini,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    // Generate content (stream)
    (
        Op::OpenAiResponseWebSocket,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GeminiLive,
        Proto::Gemini,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::OpenAi,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::Claude,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::Gemini,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::StreamGenerateContent,
        Proto::GeminiNDJson,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    // Embeddings
    (Op::Embedding, Proto::OpenAi, Op::Embedding, Proto::OpenAi),
    (Op::Embedding, Proto::Gemini, Op::Embedding, Proto::OpenAi),
    // Internal OpenAI ops routed into chat-generate
    (
        Op::Compact,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
];

pub fn default_dispatch_table() -> ProviderDispatchTable {
    let mut table = table_from_typed_routes(ROUTES);
    table.set(
        RouteKey::new(Op::CountToken, Proto::OpenAi),
        RouteImplementation::Local,
    );
    table.set(
        RouteKey::new(Op::CountToken, Proto::Claude),
        RouteImplementation::Local,
    );
    table.set(
        RouteKey::new(Op::CountToken, Proto::Gemini),
        RouteImplementation::Local,
    );
    table
}
