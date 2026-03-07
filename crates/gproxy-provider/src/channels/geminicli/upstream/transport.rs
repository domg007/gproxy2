use super::*;

pub(super) async fn send_geminicli_request(
    client: &WreqClient,
    params: GeminiCliRequestParams<'_>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let user_agent = params
        .custom_user_agent
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| geminicli_user_agent(params.model_for_ua));
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, params.extra_headers);
    add_or_replace_header(&mut headers, "accept", "application/json");
    add_or_replace_header(
        &mut headers,
        "authorization",
        format!("Bearer {}", params.access_token),
    );
    add_or_replace_header(&mut headers, "user-agent", user_agent);
    add_or_replace_header(&mut headers, "accept-encoding", "gzip");
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
