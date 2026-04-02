use crate::channel::Channel;
use crate::health::CredentialHealth;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError, UpstreamResponse};

/// Default max retries per credential when 429 has no retry-after header.
const DEFAULT_MAX_RETRIES_PER_CREDENTIAL: u32 = 3;

/// Retry a request across multiple credentials.
///
/// For each eligible credential, tries up to `max_retries` times on 429
/// without `retry-after`. If 429 includes `retry-after`, the credential
/// is marked with a cooldown and skipped immediately.
///
/// The caller provides a `send` closure that performs the actual HTTP request.
pub async fn retry_with_credentials<C, F, Fut>(
    channel: &C,
    credentials: &mut [(C::Credential, C::Health)],
    settings: &C::Settings,
    request: &PreparedRequest,
    send: F,
) -> Result<UpstreamResponse, UpstreamError>
where
    C: Channel,
    F: Fn(http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<UpstreamResponse, UpstreamError>>,
{
    retry_with_credentials_max(
        channel,
        credentials,
        settings,
        request,
        DEFAULT_MAX_RETRIES_PER_CREDENTIAL,
        send,
    )
    .await
}

/// Same as [`retry_with_credentials`] with configurable max retries.
pub async fn retry_with_credentials_max<C, F, Fut>(
    channel: &C,
    credentials: &mut [(C::Credential, C::Health)],
    settings: &C::Settings,
    request: &PreparedRequest,
    max_retries: u32,
    send: F,
) -> Result<UpstreamResponse, UpstreamError>
where
    C: Channel,
    F: Fn(http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<UpstreamResponse, UpstreamError>>,
{
    let model = request.model.as_deref();

    // Filter to eligible credentials
    let eligible: Vec<usize> = credentials
        .iter()
        .enumerate()
        .filter(|(_, (_, health))| health.is_available(model))
        .map(|(i, _)| i)
        .collect();

    if eligible.is_empty() {
        return Err(UpstreamError::NoEligibleCredentials);
    }

    let mut last_error = None;

    for &idx in &eligible {
        let mut attempts = 0u32;

        loop {
            let (credential, _) = &credentials[idx];

            // Build HTTP request
            let http_request = match channel.prepare_request(credential, settings, request) {
                Ok(req) => req,
                Err(e) => {
                    tracing::warn!("Failed to prepare request for credential {}: {}", idx, e);
                    last_error = Some(e);
                    break;
                }
            };

            // Send request
            let response = match send(http_request).await {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!("HTTP error for credential {}: {}", idx, e);
                    last_error = Some(e);
                    break;
                }
            };

            // Classify response
            let classification =
                channel.classify_response(response.status, &response.headers, &response.body);

            let (_, health) = &mut credentials[idx];
            match classification {
                ResponseClassification::Success => {
                    health.record_success(model);
                    return Ok(response);
                }
                ResponseClassification::AuthDead => {
                    health.record_error(response.status, model, None);
                    tracing::warn!("Credential {} auth dead ({})", idx, response.status);
                    break;
                }
                ResponseClassification::RateLimited { retry_after_ms } => {
                    if retry_after_ms.is_some() {
                        // Has retry-after: cooldown this credential, move to next
                        health.record_error(response.status, model, retry_after_ms);
                        tracing::info!("Credential {} rate limited with retry-after", idx);
                        break;
                    }
                    // No retry-after: retry same credential up to max_retries
                    attempts += 1;
                    if attempts >= max_retries {
                        health.record_error(response.status, model, None);
                        tracing::info!(
                            "Credential {} rate limited, exhausted {} retries",
                            idx,
                            max_retries
                        );
                        break;
                    }
                    tracing::info!(
                        "Credential {} rate limited (no retry-after), attempt {}/{}",
                        idx,
                        attempts,
                        max_retries
                    );
                    continue;
                }
                ResponseClassification::TransientError => {
                    health.record_error(response.status, model, None);
                    tracing::info!("Credential {} transient error ({})", idx, response.status);
                    break;
                }
                ResponseClassification::PermanentError => {
                    return Ok(response);
                }
            }
        }
    }

    Err(last_error.unwrap_or(UpstreamError::AllCredentialsExhausted))
}

