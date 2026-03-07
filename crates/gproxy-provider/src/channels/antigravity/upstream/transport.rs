use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_antigravity_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    access_token: &str,
    user_agent: &str,
    request_type: Option<&str>,
    session_id: Option<&str>,
    extra_headers: &[(String, String)],
    body: Option<&[u8]>,
    request_id: &str,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(&mut headers, "accept", "application/json");
    add_or_replace_header(
        &mut headers,
        "authorization",
        format!("Bearer {access_token}"),
    );
    add_or_replace_header(&mut headers, "user-agent", user_agent.to_string());
    add_or_replace_header(&mut headers, "accept-encoding", "gzip");
    add_or_replace_header(&mut headers, "requestid", request_id.to_string());
    if let Some(value) = request_type {
        add_or_replace_header(&mut headers, "requesttype", value.to_string());
    }
    if let Some(value) = session_id.map(str::trim).filter(|value| !value.is_empty()) {
        add_or_replace_header(&mut headers, "x-machine-session-id", value.to_string());
    }
    if body.is_some() {
        add_or_replace_header(&mut headers, "content-type", "application/json");
    }
    crate::channels::upstream::tracked_send_request(
        client,
        method,
        url,
        headers,
        body.map(|value| value.to_vec()),
    )
    .await
}
