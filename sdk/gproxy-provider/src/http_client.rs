use crate::response::{UpstreamError, UpstreamResponse};

/// Send an `http::Request<Vec<u8>>` via wreq and return an `UpstreamResponse`.
pub async fn send_request(
    client: &wreq::Client,
    request: http::Request<Vec<u8>>,
) -> Result<UpstreamResponse, UpstreamError> {
    let wreq_request = wreq::Request::from(request);

    let response = client
        .execute(wreq_request)
        .await
        .map_err(|e| UpstreamError::Http(e.to_string()))?;

    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(|e| UpstreamError::Http(e.to_string()))?
        .to_vec();

    Ok(UpstreamResponse {
        status,
        headers,
        body,
    })
}
