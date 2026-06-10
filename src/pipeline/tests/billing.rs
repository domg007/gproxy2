//! M6 §17 settlement integration: upstream usage on a normal stream end,
//! the counting ladder on client drop, and include_usage injection.

use super::*;
use crate::store::persistence::records::Usage;

fn openai_stream_ctx(request_id: &str, model: &str) -> RequestCtx {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-test".parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let body = json!({
        "model": model, "stream": true,
        "messages": [{ "role": "user", "content": "hi there" }]
    });
    RequestCtx {
        request_id: request_id.into(),
        method: Method::POST,
        path: "/v1/chat/completions".into(),
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

/// Settlement is detached (spawned) — poll until the usage row lands.
async fn wait_usage(state: &AppState) -> Usage {
    for _ in 0..200 {
        let rows = state.persistence.list_usages(10).await.expect("list");
        if let Some(row) = rows.into_iter().next() {
            return row;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("usage row never appeared");
}

#[tokio::test]
async fn normal_stream_settles_upstream_usage() {
    let chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":0,"model":"gpt-test","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
    let usage_chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":0,"model":"gpt-test","choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":500}}"#;
    let fake = Arc::new(FakeUpstream::new(
        Bytes::new(),
        vec![
            Bytes::from(format!("{chunk}\n\n")),
            Bytes::from(format!("{usage_chunk}\n\ndata: [DONE]\n\n")),
        ],
    ));
    let bundle = bundle_with(
        "provider_models",
        json!([{
            "id": 1, "provider_id": 1, "model_id": "gpt-test", "display_name": null,
            "pricing_json": { "input": "3", "output": "15" },
            "variants_json": null, "enabled": true
        }]),
    );
    let (state, _dir) = state_with_bundle(Arc::clone(&fake), &bundle).await;

    let outcome = crate::pipeline::execute(&state, openai_stream_ctx("bill-1", "claude-test"))
        .await
        .expect("pipeline ok");
    let ResponseBody::Stream(s) = outcome.body else {
        panic!("expected Stream")
    };
    use futures_util::StreamExt;
    let relayed: Vec<Bytes> = s.map(|r| r.expect("chunk ok")).collect().await;
    assert!(!relayed.is_empty());

    let row = wait_usage(&state).await;
    assert_eq!(row.request_id, "bill-1");
    assert_eq!(row.usage_source, "upstream");
    assert_eq!(row.ended, "complete");
    assert_eq!(row.input_tokens, 1000);
    assert_eq!(row.output_tokens, 500);
    // 1000 × 3/M + 500 × 15/M
    assert_eq!(row.cost, "0.0105".parse().unwrap());
    assert_eq!(row.model.as_deref(), Some("gpt-test"));
}

#[tokio::test]
async fn client_drop_settles_estimated() {
    let chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":0,"model":"gpt-test","choices":[{"index":0,"delta":{"content":"partial output text"},"finish_reason":null}]}"#;
    let fake = Arc::new(FakeUpstream::new(
        Bytes::new(),
        vec![
            Bytes::from(format!("{chunk}\n\n")),
            Bytes::from_static(b"data: [DONE]\n\n"),
        ],
    ));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    let outcome = crate::pipeline::execute(&state, openai_stream_ctx("bill-2", "claude-test"))
        .await
        .expect("pipeline ok");
    let ResponseBody::Stream(mut s) = outcome.body else {
        panic!("expected Stream")
    };
    use futures_util::StreamExt;
    let first = s.next().await.expect("one chunk").expect("chunk ok");
    assert!(!first.is_empty());
    drop(s); // client gone — the Drop guard settles Interrupted

    let row = wait_usage(&state).await;
    assert_eq!(row.ended, "interrupted");
    assert!(
        row.usage_source == "estimated" || row.usage_source == "counted",
        "source: {}",
        row.usage_source
    );
    assert!(row.output_tokens > 0, "buffered text counted");
    assert!(row.input_tokens > 0, "request body counted");
}

#[tokio::test]
async fn include_usage_injected() {
    let chunk = r#"data: {"id":"c","object":"chat.completion.chunk","created":0,"model":"gpt-test","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
    let fake = Arc::new(FakeUpstream::new(
        Bytes::new(),
        vec![Bytes::from(format!("{chunk}\n\ndata: [DONE]\n\n"))],
    ));
    let (state, _dir) = state_with(Arc::clone(&fake)).await;

    // transform path: claude inbound → openai-chat upstream stream
    crate::pipeline::execute(&state, claude_ctx("claude-test", true))
        .await
        .expect("transform path ok");
    // passthrough path: openai-chat inbound stream
    crate::pipeline::execute(&state, openai_stream_ctx("bill-3", "claude-test"))
        .await
        .expect("passthrough path ok");

    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for (i, s) in seen.iter().enumerate() {
        let v: Value = serde_json::from_slice(&s.body).unwrap();
        assert_eq!(
            v["stream_options"]["include_usage"], true,
            "attempt {i}: {v}"
        );
        assert_eq!(v["stream"], true, "attempt {i} still streams");
    }
}
