//! §6.3 locally-served operations: gateway model lists and local/fallback
//! token counting — no (or failed) upstream involvement.

use super::*;

#[tokio::test]
async fn aggregated_models_lists_aliases_and_routes() {
    let fake = Arc::new(FakeUpstream::new(Bytes::new(), vec![]));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-test".parse().unwrap());
    let ctx = RequestCtx {
        request_id: "t-m".into(),
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
    };

    let outcome = crate::pipeline::execute(&state, ctx).await.expect("ok");
    assert_eq!(outcome.status, StatusCode::OK);
    let ResponseBody::Full(b) = outcome.body else {
        panic!("expected Full")
    };
    let v: Value = serde_json::from_slice(&b).unwrap();
    let ids: Vec<&str> = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    for expected in ["claude-test", "claude-direct", "to-openai", "to-claude"] {
        assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
    }
    // gateway view: never touches an upstream
    assert!(fake.seen.lock().unwrap().is_empty());
}

fn count_ctx(model: &str) -> RequestCtx {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-test".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "count my tokens please" }]
    });
    RequestCtx {
        request_id: "t-c".into(),
        method: Method::POST,
        path: "/v1/messages/count_tokens".into(),
        query: None,
        headers,
        body: Bytes::from(serde_json::to_vec(&body).unwrap()),
        mode: RoutingMode::Aggregated,
        identity: None,
        op: None,
        stream: false,
        route_name: None,
    }
}

/// §6.3 default: count_tokens routed at an openai-family channel is served
/// locally — claude-shaped response, no upstream call.
#[tokio::test]
async fn count_tokens_on_openai_channel_serves_locally() {
    let fake = Arc::new(FakeUpstream::new(Bytes::new(), vec![]));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let outcome = crate::pipeline::execute(&state, count_ctx("claude-test"))
        .await
        .expect("ok");

    assert_eq!(outcome.status, StatusCode::OK);
    let ResponseBody::Full(b) = outcome.body else {
        panic!("expected Full")
    };
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert!(v["input_tokens"].as_u64().unwrap() > 0, "body: {v}");
    assert!(fake.seen.lock().unwrap().is_empty(), "no upstream call");
}

/// §6.3 fallback: when every upstream count attempt fails, the gateway answers
/// with a local count instead of a 502.
#[tokio::test]
async fn count_tokens_falls_back_to_local_when_upstream_fails() {
    let mut fake = FakeUpstream::new(Bytes::from_static(b"{}"), vec![]);
    fake.status = StatusCode::INTERNAL_SERVER_ERROR;
    let fake = Arc::new(fake);
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    // claude-direct → claude provider → native count passthrough → 500s
    let outcome = crate::pipeline::execute(&state, count_ctx("claude-direct"))
        .await
        .expect("local fallback");

    assert_eq!(outcome.status, StatusCode::OK);
    let ResponseBody::Full(b) = outcome.body else {
        panic!("expected Full")
    };
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert!(v["input_tokens"].as_u64().unwrap() > 0, "body: {v}");
    assert_eq!(fake.seen.lock().unwrap().len(), 1, "upstream was attempted");
}

/// Explicit `local` routing rule: scoped ListModels served from the snapshot's
/// exposed provider_models (manual rows + variants), no upstream call.
#[tokio::test]
async fn scoped_models_list_served_locally_via_rule() {
    let fake = Arc::new(FakeUpstream::new(Bytes::new(), vec![]));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-test".parse().unwrap());
    let ctx = RequestCtx {
        request_id: "t-lm".into(),
        method: Method::GET,
        path: "/v1/models".into(),
        query: None,
        headers,
        body: Bytes::new(),
        mode: RoutingMode::Scoped {
            provider: "oai".into(),
        },
        identity: None,
        op: None,
        stream: false,
        route_name: None,
    };

    let outcome = crate::pipeline::execute(&state, ctx).await.expect("ok");
    assert_eq!(outcome.status, StatusCode::OK);
    let ResponseBody::Full(b) = outcome.body else {
        panic!("expected Full")
    };
    let v: Value = serde_json::from_slice(&b).unwrap();
    let ids: Vec<&str> = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["gpt-test", "gpt-test-thinking"],
        "exposed rows listed"
    );
    assert!(fake.seen.lock().unwrap().is_empty(), "no upstream call");
}
