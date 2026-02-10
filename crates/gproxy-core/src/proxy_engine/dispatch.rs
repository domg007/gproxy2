use gproxy_provider_core::{DispatchRule, DispatchTable, Op, Proto, TransformContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateMode {
    Same,
    StreamToNon,
    NonToStream,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedCall {
    pub provider_proto: Proto,
    pub provider_op: Op,
    pub mode: GenerateMode,
}

fn rule_to_proto(user_proto: Proto, rule: DispatchRule) -> Option<Proto> {
    match rule {
        DispatchRule::Native => Some(user_proto),
        DispatchRule::Transform { target } => Some(target),
        DispatchRule::Unsupported => None,
    }
}

pub fn resolve_call_shape(
    dispatch: &DispatchTable,
    user_proto: Proto,
    user_op: Op,
) -> Option<ResolvedCall> {
    let is_generate = matches!(user_op, Op::GenerateContent | Op::StreamGenerateContent);
    if matches!(
        user_op,
        Op::ResponseGet
            | Op::ResponseDelete
            | Op::ResponseCancel
            | Op::ResponseListInputItems
            | Op::ResponseCompact
            | Op::MemoryTraceSummarize
    ) {
        if user_proto != Proto::OpenAI {
            return None;
        }
        return Some(ResolvedCall {
            provider_proto: user_proto,
            provider_op: user_op,
            mode: GenerateMode::Same,
        });
    }
    if !is_generate {
        let ctx = TransformContext {
            src: user_proto,
            dst: user_proto,
            src_op: user_op,
            dst_op: user_op,
        };
        let rule = dispatch.rule_for_context(&ctx);
        let provider_proto = rule_to_proto(user_proto, rule)?;
        return Some(ResolvedCall {
            provider_proto,
            provider_op: user_op,
            mode: GenerateMode::Same,
        });
    }

    // Generate ops: prefer same stream mode first, then attempt stream mismatch fallback.
    let same_ctx = TransformContext {
        src: user_proto,
        dst: user_proto,
        src_op: user_op,
        dst_op: user_op,
    };
    let same_rule = dispatch.rule_for_context(&same_ctx);
    if let Some(provider_proto) = rule_to_proto(user_proto, same_rule) {
        return Some(ResolvedCall {
            provider_proto,
            provider_op: user_op,
            mode: GenerateMode::Same,
        });
    }

    let want_stream = user_op == Op::StreamGenerateContent;
    if want_stream {
        // Non-stream -> stream fallback: call non-stream upstream and streamify to the user.
        let non_ctx = TransformContext {
            src: user_proto,
            dst: user_proto,
            src_op: Op::GenerateContent,
            dst_op: Op::GenerateContent,
        };
        let rule = dispatch.rule_for_context(&non_ctx);
        let provider_proto = rule_to_proto(user_proto, rule)?;
        return Some(ResolvedCall {
            provider_proto,
            provider_op: Op::GenerateContent,
            mode: GenerateMode::NonToStream,
        });
    }

    // Stream -> non-stream fallback: call stream upstream and aggregate to non-stream.
    let stream_ctx = TransformContext {
        src: user_proto,
        dst: user_proto,
        src_op: Op::StreamGenerateContent,
        dst_op: Op::StreamGenerateContent,
    };
    let rule = dispatch.rule_for_context(&stream_ctx);
    let provider_proto = rule_to_proto(user_proto, rule)?;
    Some(ResolvedCall {
        provider_proto,
        provider_op: Op::StreamGenerateContent,
        mode: GenerateMode::StreamToNon,
    })
}
