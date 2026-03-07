use super::*;

pub(super) async fn send_codex_request(
    client: &WreqClient,
    params: CodexRequestParams<'_>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, params.extra_headers);
    add_or_replace_header(
        &mut headers,
        "authorization",
        format!("Bearer {}", params.access_token),
    );
    add_or_replace_header(
        &mut headers,
        ACCOUNT_ID_HEADER,
        params.account_id.to_string(),
    );
    add_or_replace_header(&mut headers, ORIGINATOR_HEADER, ORIGINATOR_VALUE);
    add_or_replace_header(
        &mut headers,
        USER_AGENT_HEADER,
        params.user_agent.to_string(),
    );
    if params.body.is_some() {
        add_or_replace_header(&mut headers, "content-type", "application/json");
    }
    crate::channels::upstream::tracked_send_request(
        client,
        params.method,
        params.url,
        headers,
        params.body.map(|value| value.to_vec()),
    )
    .await
}

pub(super) async fn send_codex_usage_request(
    client: &WreqClient,
    url: &str,
    access_token: &str,
    account_id: &str,
    user_agent: &str,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {access_token}"),
        ),
        (ACCOUNT_ID_HEADER.to_string(), account_id.to_string()),
        (ORIGINATOR_HEADER.to_string(), ORIGINATOR_VALUE.to_string()),
        (USER_AGENT_HEADER.to_string(), user_agent.to_string()),
        ("accept".to_string(), "application/json".to_string()),
    ];
    crate::channels::upstream::tracked_send_request(client, WreqMethod::GET, url, headers, None)
        .await
}
