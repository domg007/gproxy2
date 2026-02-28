use std::future::Future;

use rand::RngExt as _;

use crate::channels::upstream::{UpstreamError, UpstreamRequestMeta};
use crate::credential::normalize_model_cooldown_key;
use crate::{ChannelCredentialStateStore, CredentialRef, ProviderDefinition};

pub enum CredentialRetryDecision<T> {
    Return(T),
    Retry {
        last_status: Option<u16>,
        last_error: Option<String>,
        last_request_meta: Option<UpstreamRequestMeta>,
    },
}

pub struct CredentialAttempt<Material> {
    pub credential_id: i64,
    pub material: Material,
    pub attempts: usize,
}

struct CredentialCandidate<Material> {
    credential_id: i64,
    material: Material,
}

pub async fn retry_with_eligible_credentials<Material, T, Select, Attempt, Fut>(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    model: Option<&str>,
    now_unix_ms: u64,
    mut select_material: Select,
    mut attempt: Attempt,
) -> Result<T, UpstreamError>
where
    Select: FnMut(&CredentialRef) -> Option<Material>,
    Attempt: FnMut(CredentialAttempt<Material>) -> Fut,
    Fut: Future<Output = CredentialRetryDecision<T>>,
{
    let normalized_model = model.and_then(normalize_model_cooldown_key);

    let mut remaining = credential_states
        .eligible_credentials(
            &provider.channel,
            provider.credentials.list_credentials(),
            normalized_model.as_deref(),
            now_unix_ms,
        )
        .into_iter()
        .filter_map(|credential| {
            select_material(credential).map(|material| CredentialCandidate {
                credential_id: credential.id,
                material,
            })
        })
        .collect::<Vec<_>>();

    if remaining.is_empty() {
        return Err(UpstreamError::NoEligibleCredential {
            channel: provider.channel.as_str().to_string(),
            model: normalized_model,
        });
    }

    let mut attempts = 0usize;
    let mut last_credential_id = None;
    let mut last_status = None;
    let mut last_error = None;
    let mut last_request_meta = None;
    while !remaining.is_empty() {
        let idx = rand::rng().random_range(0..remaining.len());
        let candidate = remaining.swap_remove(idx);
        attempts += 1;
        match attempt(CredentialAttempt {
            credential_id: candidate.credential_id,
            material: candidate.material,
            attempts,
        })
        .await
        {
            CredentialRetryDecision::Return(value) => return Ok(value),
            CredentialRetryDecision::Retry {
                last_status: status,
                last_error: error,
                last_request_meta: request_meta,
            } => {
                last_credential_id = Some(candidate.credential_id);
                last_status = status;
                last_error = error;
                last_request_meta = request_meta;
            }
        }
    }

    Err(UpstreamError::AllCredentialsExhausted {
        channel: provider.channel.as_str().to_string(),
        attempts,
        last_credential_id,
        last_status,
        last_error,
        last_request_meta: last_request_meta.map(Box::new),
    })
}
