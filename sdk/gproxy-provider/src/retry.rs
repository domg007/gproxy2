use crate::channel::Channel;
use crate::health::CredentialHealth;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError, UpstreamResponse};

/// Retry a request across multiple credentials.
///
/// Filters credentials by health, then tries each one in order:
/// - On success: records success, returns response
/// - On auth dead: marks credential dead, tries next
/// - On rate limit: marks model cooldown, tries next
/// - On transient error: tries next
/// - On permanent error: returns immediately
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
        let (credential, _) = &credentials[idx];

        // Build HTTP request
        let http_request = match channel.prepare_request(credential, settings, request) {
            Ok(req) => req,
            Err(e) => {
                tracing::warn!("Failed to prepare request for credential {}: {}", idx, e);
                last_error = Some(e);
                continue;
            }
        };

        // Send request
        let response = match send(http_request).await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!("HTTP error for credential {}: {}", idx, e);
                last_error = Some(e);
                continue;
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
                continue;
            }
            ResponseClassification::RateLimited { retry_after_ms } => {
                health.record_error(response.status, model, retry_after_ms);
                tracing::info!("Credential {} rate limited", idx);
                continue;
            }
            ResponseClassification::TransientError => {
                health.record_error(response.status, model, None);
                tracing::info!("Credential {} transient error ({})", idx, response.status);
                continue;
            }
            ResponseClassification::PermanentError => {
                return Ok(response);
            }
        }
    }

    Err(last_error.unwrap_or(UpstreamError::AllCredentialsExhausted))
}
