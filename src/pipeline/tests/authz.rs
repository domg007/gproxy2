//! M3 authz integration: enforcement in the aggregated arm + permission-
//! filtered model listing. Each test builds its own state (fresh MemoryCache
//! + per-test bundle), so counters and grants never leak between tests.

use super::*;
use crate::pipeline::error::PipelineError;

fn chat_ok() -> Bytes {
    let body = json!({
        "id": "chatcmpl-1", "object": "chat.completion", "created": 0, "model": "gpt-test",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
    });
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

/// `expect_err` without requiring `ExecOutcome: Debug`.
async fn exec_err(state: &AppState, ctx: RequestCtx) -> PipelineError {
    match crate::pipeline::execute(state, ctx).await {
        Err(e) => e,
        Ok(_) => panic!("expected pipeline error"),
    }
}

fn models_ctx(api_key: &str) -> RequestCtx {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {api_key}").parse().unwrap(),
    );
    RequestCtx {
        request_id: "t-az".into(),
        method: Method::GET,
        path: "/v1/models".into(),
        query: None,
        headers,
        body: Bytes::new(),
        mode: RoutingMode::Aggregated,
        identity: None,
        op: None,
        stream: false,
        route_name: None,
        pending_micros: 0,
    }
}

#[tokio::test]
async fn no_permission_user_403() {
    let fake = Arc::new(FakeUpstream::new(chat_ok(), vec![]));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let err = exec_err(&state, claude_ctx_as("sk-noperm", "claude-test", false)).await;
    assert!(matches!(err, PipelineError::Forbidden), "got {err:?}");
    assert!(fake.seen.lock().unwrap().is_empty(), "no upstream call");
}

#[tokio::test]
async fn rpm_limit_trips() {
    let bundle = bundle_with(
        "rate_limits",
        json!([{ "id": 1, "scope": "user", "scope_id": 1, "route_pattern": "*",
                 "rpm": 1, "rpd": null, "total_tokens": null }]),
    );
    let fake = Arc::new(FakeUpstream::new(chat_ok(), vec![]));
    let (state, _dir) = state_with_bundle(Arc::clone(&fake), &bundle).await;

    crate::pipeline::execute(&state, claude_ctx("claude-test", false))
        .await
        .expect("first request under limit");
    let err = exec_err(&state, claude_ctx("claude-test", false)).await;
    assert!(
        matches!(err, PipelineError::RateLimited { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn quota_exceeded_429() {
    let bundle = bundle_with(
        "quotas",
        json!([{ "id": 1, "scope": "user", "scope_id": 1,
                 "quota_total": "1.00", "cost_used": "2.00" }]),
    );
    let fake = Arc::new(FakeUpstream::new(chat_ok(), vec![]));
    let (state, _dir) = state_with_bundle(Arc::clone(&fake), &bundle).await;

    let err = exec_err(&state, claude_ctx("claude-test", false)).await;
    assert!(matches!(err, PipelineError::QuotaExceeded), "got {err:?}");
    assert!(fake.seen.lock().unwrap().is_empty(), "no upstream call");
}

#[tokio::test]
async fn models_list_filtered() {
    let fake = Arc::new(FakeUpstream::new(Bytes::new(), vec![]));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let list = |outcome: crate::pipeline::ExecOutcome| -> usize {
        let ResponseBody::Full(b) = outcome.body else {
            panic!("expected Full")
        };
        let v: Value = serde_json::from_slice(&b).unwrap();
        v["data"].as_array().unwrap().len()
    };

    let denied = crate::pipeline::execute(&state, models_ctx("sk-noperm"))
        .await
        .expect("listing itself is allowed");
    assert_eq!(denied.status, StatusCode::OK);
    assert_eq!(list(denied), 0, "noperm sees nothing");

    let allowed = crate::pipeline::execute(&state, models_ctx("sk-test"))
        .await
        .expect("ok");
    assert!(list(allowed) >= 2, "grant holder sees aliases + routes");
}
