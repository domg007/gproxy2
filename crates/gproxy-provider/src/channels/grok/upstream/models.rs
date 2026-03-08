use super::response::{build_json_http_response, openai_error_body};
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct GrokModelSpec {
    pub(super) public_id: &'static str,
    pub(super) upstream_model: &'static str,
    pub(super) upstream_mode: Option<&'static str>,
    pub(super) created: u64,
}

pub(super) const GROK_MODELS: &[GrokModelSpec] = &[
    GrokModelSpec {
        public_id: "grok-3",
        upstream_model: "grok-3",
        upstream_mode: Some("MODEL_MODE_GROK_3"),
        created: 1_735_689_600,
    },
    GrokModelSpec {
        public_id: "grok-3-mini",
        upstream_model: "grok-3",
        upstream_mode: Some("MODEL_MODE_GROK_3_MINI_THINKING"),
        created: 1_735_689_601,
    },
    GrokModelSpec {
        public_id: "grok-3-thinking",
        upstream_model: "grok-3",
        upstream_mode: Some("MODEL_MODE_GROK_3_THINKING"),
        created: 1_735_689_602,
    },
    GrokModelSpec {
        public_id: "grok-4",
        upstream_model: "grok-4",
        upstream_mode: Some("MODEL_MODE_GROK_4"),
        created: 1_735_689_603,
    },
    GrokModelSpec {
        public_id: "grok-4-mini",
        upstream_model: "grok-4-mini",
        upstream_mode: Some("MODEL_MODE_GROK_4_MINI_THINKING"),
        created: 1_735_689_604,
    },
    GrokModelSpec {
        public_id: "grok-4-thinking",
        upstream_model: "grok-4",
        upstream_mode: Some("MODEL_MODE_GROK_4_THINKING"),
        created: 1_735_689_605,
    },
    GrokModelSpec {
        public_id: "grok-4-heavy",
        upstream_model: "grok-4",
        upstream_mode: Some("MODEL_MODE_HEAVY"),
        created: 1_735_689_606,
    },
    GrokModelSpec {
        public_id: "grok-4.1-mini",
        upstream_model: "grok-4-1-thinking-1129",
        upstream_mode: Some("MODEL_MODE_GROK_4_1_MINI_THINKING"),
        created: 1_735_689_607,
    },
    GrokModelSpec {
        public_id: "grok-4.1-fast",
        upstream_model: "grok-4-1-thinking-1129",
        upstream_mode: Some("MODEL_MODE_FAST"),
        created: 1_735_689_608,
    },
    GrokModelSpec {
        public_id: "grok-4.1-expert",
        upstream_model: "grok-4-1-thinking-1129",
        upstream_mode: Some("MODEL_MODE_EXPERT"),
        created: 1_735_689_609,
    },
    GrokModelSpec {
        public_id: "grok-4.1-thinking",
        upstream_model: "grok-4-1-thinking-1129",
        upstream_mode: Some("MODEL_MODE_GROK_4_1_THINKING"),
        created: 1_735_689_610,
    },
    GrokModelSpec {
        public_id: "grok-4.20-beta",
        upstream_model: "grok-420",
        upstream_mode: Some("MODEL_MODE_GROK_420"),
        created: 1_735_689_611,
    },
    GrokModelSpec {
        public_id: "grok-imagine-1.0",
        upstream_model: "grok-imagine-1.0",
        upstream_mode: Some("MODEL_MODE_FAST"),
        created: 1_735_689_612,
    },
    GrokModelSpec {
        public_id: "grok-imagine-1.0-video",
        upstream_model: "grok-imagine-1.0-video",
        upstream_mode: Some("MODEL_MODE_FAST"),
        created: 1_735_689_613,
    },
];

pub(super) fn resolve_requested_model(request_model: &str) -> GrokResolvedModel {
    let model = request_model.trim().to_string();
    if let Some(spec) = GROK_MODELS
        .iter()
        .find(|item| item.public_id.eq_ignore_ascii_case(model.as_str()))
    {
        return GrokResolvedModel {
            request_model: spec.public_id.to_string(),
            upstream_model: spec.upstream_model.to_string(),
            upstream_mode: spec.upstream_mode.map(ToOwned::to_owned),
        };
    }
    GrokResolvedModel {
        request_model: model.clone(),
        upstream_model: model,
        upstream_mode: None,
    }
}

pub(super) fn build_model_list_http_response() -> Result<wreq::Response, UpstreamError> {
    build_json_http_response(
        StatusCode::OK,
        &json!({
            "object": "list",
            "data": GROK_MODELS
                .iter()
                .map(|model| {
                    json!({
                        "id": model.public_id,
                        "created": model.created,
                        "object": "model",
                        "owned_by": "xai",
                    })
                })
                .collect::<Vec<_>>(),
        }),
    )
}

pub(super) fn build_model_get_http_response(target: &str) -> Result<wreq::Response, UpstreamError> {
    match GROK_MODELS
        .iter()
        .find(|model| model.public_id.eq_ignore_ascii_case(target.trim()))
    {
        Some(model) => build_json_http_response(
            StatusCode::OK,
            &json!({
                "id": model.public_id,
                "created": model.created,
                "object": "model",
                "owned_by": "xai",
            }),
        ),
        None => build_json_http_response(
            StatusCode::NOT_FOUND,
            &openai_error_body(
                format!("model '{}' not found", target.trim()),
                "invalid_request_error",
                Some("model"),
                Some("model_not_found"),
            ),
        ),
    }
}
