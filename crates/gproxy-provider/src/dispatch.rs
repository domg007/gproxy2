use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRoute};
use serde::{Deserialize, Serialize};

use crate::{channel::BuiltinChannel, channels};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouteKey {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
}

impl RouteKey {
    pub const fn new(operation: OperationFamily, protocol: ProtocolKind) -> Self {
        Self {
            operation,
            protocol,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteImplementation {
    Passthrough,
    TransformTo { destination: RouteKey },
    Local,
    Unsupported,
}

impl RouteImplementation {
    pub const fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    pub const fn is_passthrough(&self) -> bool {
        matches!(self, Self::Passthrough)
    }

    pub const fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchRule {
    pub route: RouteKey,
    pub implementation: RouteImplementation,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderDispatchTable {
    pub rules: Vec<DispatchRule>,
}

impl ProviderDispatchTable {
    pub fn resolve(&self, route: RouteKey) -> Option<&RouteImplementation> {
        self.rules
            .iter()
            .find(|rule| rule.route == route)
            .map(|rule| &rule.implementation)
    }

    pub fn set(&mut self, route: RouteKey, implementation: RouteImplementation) {
        if let Some(rule) = self.rules.iter_mut().find(|rule| rule.route == route) {
            rule.implementation = implementation;
            return;
        }
        self.rules.push(DispatchRule {
            route,
            implementation,
        });
    }

    pub fn resolve_src_dst(&self, src: RouteKey) -> Option<(RouteKey, RouteKey)> {
        let implementation = self.resolve(src)?;
        let dst = match implementation {
            RouteImplementation::TransformTo { destination } => *destination,
            RouteImplementation::Passthrough => src,
            RouteImplementation::Local => return None,
            RouteImplementation::Unsupported => return None,
        };
        Some((src, dst))
    }

    pub fn resolve_transform_route(&self, src: RouteKey) -> Option<TransformRoute> {
        let (src, dst) = self.resolve_src_dst(src)?;
        Some(TransformRoute {
            src_operation: src.operation,
            src_protocol: src.protocol,
            dst_operation: dst.operation,
            dst_protocol: dst.protocol,
        })
    }

    pub fn default_for_builtin(channel: BuiltinChannel) -> Self {
        match channel {
            BuiltinChannel::OpenAi => channels::openai::default_dispatch_table(),
            BuiltinChannel::Claude => channels::claude::default_dispatch_table(),
            BuiltinChannel::AiStudio => channels::aistudio::default_dispatch_table(),
            BuiltinChannel::VertexExpress => channels::vertexexpress::default_dispatch_table(),
            BuiltinChannel::Vertex => channels::vertex::default_dispatch_table(),
            BuiltinChannel::GeminiCli => channels::geminicli::default_dispatch_table(),
            BuiltinChannel::ClaudeCode => channels::claudecode::default_dispatch_table(),
            BuiltinChannel::Codex => channels::codex::default_dispatch_table(),
            BuiltinChannel::Antigravity => channels::antigravity::default_dispatch_table(),
            BuiltinChannel::Nvidia => channels::nvidia::default_dispatch_table(),
            BuiltinChannel::Deepseek => channels::deepseek::default_dispatch_table(),
        }
    }

    pub fn default_for_custom() -> Self {
        use gproxy_middleware::OperationFamily as Op;
        use gproxy_middleware::ProtocolKind as Proto;

        let mut table = Self::default();
        // Model list/get: only OpenAI is native, other protocols are transformed.
        table.set(
            RouteKey::new(Op::ModelList, Proto::OpenAi),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new(Op::ModelList, Proto::Claude),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::ModelList, Proto::OpenAi),
            },
        );
        table.set(
            RouteKey::new(Op::ModelList, Proto::Gemini),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::ModelList, Proto::OpenAi),
            },
        );
        table.set(
            RouteKey::new(Op::ModelGet, Proto::OpenAi),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new(Op::ModelGet, Proto::Claude),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::ModelGet, Proto::OpenAi),
            },
        );
        table.set(
            RouteKey::new(Op::ModelGet, Proto::Gemini),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::ModelGet, Proto::OpenAi),
            },
        );

        // Generate content: chat completions is native, others transform to it.
        table.set(
            RouteKey::new(Op::GenerateContent, Proto::OpenAi),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::GenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table.set(
            RouteKey::new(Op::GenerateContent, Proto::OpenAiChatCompletion),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new(Op::GenerateContent, Proto::Claude),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::GenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table.set(
            RouteKey::new(Op::GenerateContent, Proto::Gemini),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::GenerateContent, Proto::OpenAiChatCompletion),
            },
        );

        // Stream generate content: chat completions is native, others transform to it.
        table.set(
            RouteKey::new(Op::StreamGenerateContent, Proto::OpenAi),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::StreamGenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table.set(
            RouteKey::new(Op::StreamGenerateContent, Proto::OpenAiChatCompletion),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new(Op::StreamGenerateContent, Proto::Claude),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::StreamGenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table.set(
            RouteKey::new(Op::StreamGenerateContent, Proto::Gemini),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::StreamGenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table.set(
            RouteKey::new(Op::StreamGenerateContent, Proto::GeminiNDJson),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::StreamGenerateContent, Proto::OpenAiChatCompletion),
            },
        );

        // Count tokens: always local.
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

        // OpenAI internal ops route into chat-completions generate.
        table.set(
            RouteKey::new(Op::Compact, Proto::OpenAi),
            RouteImplementation::TransformTo {
                destination: RouteKey::new(Op::GenerateContent, Proto::OpenAiChatCompletion),
            },
        );
        table
    }
}

pub type TypedRoute = (OperationFamily, ProtocolKind, OperationFamily, ProtocolKind);

pub fn table_from_typed_routes(routes: &[TypedRoute]) -> ProviderDispatchTable {
    let mut table = ProviderDispatchTable::default();
    for &(src_op, src_proto, dst_op, dst_proto) in routes {
        let src = RouteKey::new(src_op, src_proto);
        let dst = RouteKey::new(dst_op, dst_proto);
        let implementation = if src == dst {
            RouteImplementation::Passthrough
        } else {
            RouteImplementation::TransformTo { destination: dst }
        };
        table.set(src, implementation);
    }
    table
}
