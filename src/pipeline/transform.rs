//! M2 transform-dispatch step: per-candidate plan (passthrough vs transform),
//! effective upstream request parts (path/query/body/headers incl. process
//! rules + model rewrite), and response-direction conversion.

use std::collections::HashMap;

use bytes::Bytes;
use http::HeaderMap;

use crate::app::snapshot::ControlPlaneSnapshot;
use crate::pipeline::classify::peek_model;
use crate::pipeline::context::{Candidate, RequestCtx};
use crate::pipeline::error::PipelineError;
use crate::process;
use crate::protocol::{self, ContentGenerationKind, OperationKey, OperationKind};
use crate::transform::routing::RoutingDecision;
use crate::transform::stream_adapter::SseTransformer;
use crate::transform::{self, TransformContext, TransformError, TransformPair, dispatch, routing};

/// Per-candidate transform plan. `Local`/`Unsupported` decisions surface as
/// errors from [`plan_for`], not as variants — the loop treats them per-policy.
#[derive(Debug, Clone)]
pub enum TransformPlan {
    Passthrough,
    Transform {
        /// inbound → upstream
        request_pair: TransformPair,
        /// upstream → inbound
        response_pair: TransformPair,
        source: OperationKey,
        target: OperationKey,
    },
}

impl TransformPlan {
    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }
}

/// Resolve the plan for one candidate.
pub fn plan_for(
    cp: &ControlPlaneSnapshot,
    provider_id: i64,
    source: OperationKey,
    target_kind: ContentGenerationKind,
) -> Result<TransformPlan, PipelineError> {
    let rules = cp
        .routing_rules_by_provider
        .get(&provider_id)
        .map(|r| r.as_slice())
        .unwrap_or(&[]);
    match routing::decide(rules, source, target_kind) {
        RoutingDecision::Passthrough => Ok(TransformPlan::Passthrough),
        RoutingDecision::Local => Err(PipelineError::LocalUnimplemented),
        RoutingDecision::Unsupported => Err(PipelineError::RuleUnsupported),
        RoutingDecision::TransformTo(target) if target == source => Ok(TransformPlan::Passthrough),
        RoutingDecision::TransformTo(target) => {
            let request_pair =
                transform::resolve(source, target).map_err(PipelineError::TransformRequest)?;
            let response_pair =
                transform::resolve(target, source).map_err(PipelineError::TransformRequest)?;
            if !dispatch::is_wired(request_pair) || !dispatch::is_wired(response_pair) {
                return Err(PipelineError::TransformRequest(
                    TransformError::InvalidInput {
                        reason: "pair not wired for bytes dispatch".to_owned(),
                    },
                ));
            }
            Ok(TransformPlan::Transform {
                request_pair,
                response_pair,
                source,
                target,
            })
        }
    }
}

/// Effective upstream request pieces for one attempt.
pub struct RequestParts {
    pub method: http::Method,
    pub path: String,
    pub query: Option<String>,
    pub body: Bytes,
    /// `Some` when process rules touched headers; otherwise use `ctx.headers`.
    pub headers: Option<HeaderMap>,
}

/// Cross-attempt memo: transformed bodies keyed by (target kind, model), plus
/// the lazily-peeked inbound model.
#[derive(Default)]
pub struct AttemptMemo {
    bodies: HashMap<(ContentGenerationKind, String), Bytes>,
    inbound_model: Option<Option<String>>,
}

impl AttemptMemo {
    fn inbound_model(&mut self, body: &Bytes) -> Option<String> {
        self.inbound_model
            .get_or_insert_with(|| peek_model(body))
            .clone()
    }
}

/// Build the effective request for one candidate: transform (memoized per
/// (kind, model)), model rewrite, endpoint synthesis, then process rules on
/// the provider-native result.
pub fn request_parts(
    ctx: &RequestCtx,
    cand: &Candidate,
    plan: &TransformPlan,
    rules: Option<&[process::CompiledRule]>,
    memo: &mut AttemptMemo,
) -> Result<RequestParts, PipelineError> {
    let op = ctx.op.expect("classified before failover");
    let (mut parts, target_key) = match plan {
        TransformPlan::Passthrough => {
            let mut path = ctx.path.clone();
            let mut query = ctx.query.clone();
            let mut body = ctx.body.clone();
            // Aggregated-mode member model rewrite. Scoped mode peeked the
            // same model into upstream_model_id, so this stays a no-op there
            // (single memoized model peek; no transform). Body-less ops
            // (models GETs) carry nothing to peek or patch.
            if op.operation.has_request_body()
                && !cand.upstream_model_id.is_empty()
                && memo.inbound_model(&ctx.body).as_deref() != Some(cand.upstream_model_id.as_str())
            {
                match op.kind {
                    OperationKind::ContentGeneration(
                        ContentGenerationKind::GeminiGenerateContent,
                    ) => {
                        // gemini carries the model in the PATH, not the body
                        let t = protocol::request_target(op, &cand.upstream_model_id, ctx.stream);
                        path = t.path;
                        if let Some(extra) = t.query {
                            query = Some(merge_query(query.as_deref(), &extra));
                        }
                    }
                    OperationKind::ContentGeneration(_) => {
                        // passthrough bodies already carry the correct stream
                        // flag; never inject it here
                        body = patch_body(&body, Some(&cand.upstream_model_id), false)?;
                    }
                    // non-content body-model rewrite (embeddings etc.) deferred
                    OperationKind::Provider(_) => {}
                }
            }
            (
                RequestParts {
                    method: ctx.method.clone(),
                    path,
                    query,
                    body,
                    headers: None,
                },
                op,
            )
        }
        TransformPlan::Transform {
            request_pair,
            source,
            target,
            ..
        } => {
            // Body-less ops (models GETs): nothing to transform or patch;
            // only endpoint synthesis applies.
            if !target.operation.has_request_body() {
                let t = protocol::request_target(*target, &cand.upstream_model_id, ctx.stream);
                (
                    RequestParts {
                        method: t.method.into(),
                        path: t.path,
                        query: t.query,
                        body: ctx.body.clone(),
                        headers: None,
                    },
                    *target,
                )
            } else {
                let OperationKind::ContentGeneration(tk) = target.kind else {
                    return Err(PipelineError::TransformRequest(
                        TransformError::InvalidInput {
                            reason: "non-content transform target (not wired in M2)".to_owned(),
                        },
                    ));
                };
                let key = (tk, cand.upstream_model_id.clone());
                let body = match memo.bodies.get(&key) {
                    Some(b) => b.clone(),
                    None => {
                        let fwd = TransformContext::new(*source, *target);
                        let converted = dispatch::request_bytes(*request_pair, &fwd, &ctx.body)
                            .map_err(PipelineError::TransformRequest)?;
                        let mut converted = Bytes::from(converted);
                        // Gemini targets keep model + streaming in the URL
                        // (request_target); body stays untouched.
                        if tk != ContentGenerationKind::GeminiGenerateContent {
                            let model = (!cand.upstream_model_id.is_empty())
                                .then_some(cand.upstream_model_id.as_str());
                            if model.is_some() || ctx.stream {
                                converted = patch_body(&converted, model, ctx.stream)?;
                            }
                        }
                        memo.bodies.insert(key, converted.clone());
                        converted
                    }
                };
                let t = protocol::request_target(*target, &cand.upstream_model_id, ctx.stream);
                (
                    RequestParts {
                        method: t.method.into(),
                        path: t.path,
                        query: t.query,
                        body,
                        headers: None,
                    },
                    *target,
                )
            }
        }
    };

    // process rules act on the provider-native request
    if let Some(rules) = rules.filter(|r| !r.is_empty()) {
        let kind = match target_key.kind {
            OperationKind::ContentGeneration(k) => Some(k),
            OperationKind::Provider(_) => None,
        };
        let mut headers = ctx.headers.clone();
        parts.body = process::apply(
            rules,
            target_key,
            kind,
            &cand.upstream_model_id,
            &mut headers,
            parts.body,
        );
        parts.headers = Some(headers);
    }
    Ok(parts)
}

/// Convert a buffered success response back to the inbound protocol.
pub fn response_body(plan: &TransformPlan, body: Bytes) -> Result<Bytes, PipelineError> {
    match plan {
        TransformPlan::Passthrough => Ok(body),
        TransformPlan::Transform {
            response_pair,
            source,
            target,
            ..
        } => {
            let rev = TransformContext::new(*target, *source);
            dispatch::response_bytes(*response_pair, &rev, &body)
                .map(Bytes::from)
                .map_err(PipelineError::TransformResponse)
        }
    }
}

/// Build the streaming adapter for a Transform plan (None for passthrough).
pub fn stream_transformer(plan: &TransformPlan) -> Option<SseTransformer> {
    match plan {
        TransformPlan::Passthrough => None,
        TransformPlan::Transform {
            response_pair,
            source,
            target,
            ..
        } => {
            let OperationKind::ContentGeneration(inbound) = source.kind else {
                return None;
            };
            Some(SseTransformer::new(
                *response_pair,
                TransformContext::new(*target, *source),
                inbound,
            ))
        }
    }
}

/// Patch the transformed provider-native body in one parse: set the member
/// model (body-model kinds) and, when the inbound request streams, the
/// `stream` flag — gemini sources carry streaming in the URL, not the body,
/// so the converted body would otherwise silently request a non-streaming
/// upstream response.
fn patch_body(body: &Bytes, model: Option<&str>, stream: bool) -> Result<Bytes, PipelineError> {
    let mut v: serde_json::Value = serde_json::from_slice(body).map_err(|e| {
        PipelineError::TransformRequest(TransformError::InvalidInput {
            reason: format!("body patch: body is not JSON: {e}"),
        })
    })?;
    if let Some(obj) = v.as_object_mut() {
        if let Some(model) = model {
            obj.insert(
                "model".to_owned(),
                serde_json::Value::String(model.to_owned()),
            );
        }
        if stream {
            obj.insert("stream".to_owned(), serde_json::Value::Bool(true));
        }
    }
    serde_json::to_vec(&v).map(Bytes::from).map_err(|e| {
        PipelineError::TransformRequest(TransformError::Serialization {
            reason: e.to_string(),
        })
    })
}

fn merge_query(existing: Option<&str>, extra: &str) -> String {
    match existing {
        Some(q) if q.split('&').any(|p| p == extra) => q.to_owned(),
        Some(q) if !q.is_empty() => format!("{q}&{extra}"),
        _ => extra.to_owned(),
    }
}
