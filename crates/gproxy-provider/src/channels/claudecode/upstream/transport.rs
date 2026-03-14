use super::*;

pub(super) async fn send_claudecode_request(
    client: &WreqClient,
    params: ClaudeCodeRequestParams<'_>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let beta_values = normalized_claudecode_beta_values(
        &[],
        params
            .request_headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
    );

    let mut sent_headers = Vec::new();
    merge_extra_headers(&mut sent_headers, params.extra_headers);
    add_or_replace_header(
        &mut sent_headers,
        "authorization",
        format!("Bearer {}", params.access_token),
    );
    add_or_replace_header(
        &mut sent_headers,
        "user-agent",
        params.user_agent.to_string(),
    );
    add_or_replace_header(&mut sent_headers, "anthropic-beta", beta_values.join(","));
    for (name, value) in params.request_headers {
        if name.eq_ignore_ascii_case("anthropic-beta") {
            continue;
        }
        add_or_replace_header(&mut sent_headers, name, value.clone());
    }
    if params.body.is_some() {
        add_or_replace_header(&mut sent_headers, "content-type", "application/json");
    }

    crate::channels::upstream::tracked_send_request(
        client,
        params.method,
        params.url,
        sent_headers,
        params.body.map(|value| value.to_vec()),
    )
    .await
}

pub(super) async fn send_claudecode_usage_request(
    client: &WreqClient,
    usage_url: &str,
    access_token: &str,
    user_agent: &str,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let sent_headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {}", access_token),
        ),
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("user-agent".to_string(), user_agent.to_string()),
        ("anthropic-beta".to_string(), OAUTH_BETA.to_string()),
    ];
    crate::channels::upstream::tracked_send_request(
        client,
        WreqMethod::GET,
        usage_url,
        sent_headers,
        None,
    )
    .await
}
