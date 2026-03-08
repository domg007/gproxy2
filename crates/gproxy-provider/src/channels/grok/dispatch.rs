use gproxy_middleware::{OperationFamily as Op, ProtocolKind as Proto};

use crate::dispatch::{
    ProviderDispatchTable, RouteImplementation, RouteKey, TypedRoute, table_from_typed_routes,
};

const ROUTES: &[TypedRoute] = &[
    (Op::ModelList, Proto::OpenAi, Op::ModelList, Proto::OpenAi),
    (Op::ModelList, Proto::Claude, Op::ModelList, Proto::OpenAi),
    (Op::ModelList, Proto::Gemini, Op::ModelList, Proto::OpenAi),
    (Op::ModelGet, Proto::OpenAi, Op::ModelGet, Proto::OpenAi),
    (Op::ModelGet, Proto::Claude, Op::ModelGet, Proto::OpenAi),
    (Op::ModelGet, Proto::Gemini, Op::ModelGet, Proto::OpenAi),
    (
        Op::CreateVideo,
        Proto::OpenAi,
        Op::CreateVideo,
        Proto::OpenAi,
    ),
    (Op::VideoGet, Proto::OpenAi, Op::VideoGet, Proto::OpenAi),
    (
        Op::VideoContentGet,
        Proto::OpenAi,
        Op::VideoContentGet,
        Proto::OpenAi,
    ),
    (
        Op::GenerateContent,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
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
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GenerateContent,
        Proto::Gemini,
        Op::GenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::OpenAiResponseWebSocket,
        Proto::OpenAi,
        Op::StreamGenerateContent,
        Proto::OpenAiChatCompletion,
    ),
    (
        Op::GeminiLive,
        Proto::Gemini,
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
        Proto::OpenAiChatCompletion,
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
    (
        Op::CreateImage,
        Proto::OpenAi,
        Op::CreateImage,
        Proto::OpenAi,
    ),
    (
        Op::StreamCreateImage,
        Proto::OpenAi,
        Op::StreamCreateImage,
        Proto::OpenAi,
    ),
    (
        Op::CreateImageEdit,
        Proto::OpenAi,
        Op::GenerateContent,
        Proto::OpenAi,
    ),
    (
        Op::StreamCreateImageEdit,
        Proto::OpenAi,
        Op::StreamGenerateContent,
        Proto::OpenAi,
    ),
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
    table.set(
        RouteKey::new(Op::Embedding, Proto::OpenAi),
        RouteImplementation::Unsupported,
    );
    table.set(
        RouteKey::new(Op::Embedding, Proto::Gemini),
        RouteImplementation::Unsupported,
    );
    table.set(
        RouteKey::new(Op::CreateImageEdit, Proto::OpenAi),
        RouteImplementation::Unsupported,
    );
    table.set(
        RouteKey::new(Op::StreamCreateImageEdit, Proto::OpenAi),
        RouteImplementation::Unsupported,
    );
    table
}
