use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::{CredentialHealth, ModelCooldown};

pub fn credential_kind_for_storage(credential: &ChannelCredential) -> String {
    match credential {
        ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(_)) => "builtin/openai",
        ChannelCredential::Builtin(BuiltinChannelCredential::Claude(_)) => "builtin/claude",
        ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(_)) => "builtin/aistudio",
        ChannelCredential::Builtin(BuiltinChannelCredential::VertexExpress(_)) => {
            "builtin/vertexexpress"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(_)) => "builtin/vertex",
        ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(_)) => "builtin/geminicli",
        ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(_)) => "builtin/claudecode",
        ChannelCredential::Builtin(BuiltinChannelCredential::Codex(_)) => "builtin/codex",
        ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(_)) => {
            "builtin/antigravity"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Nvidia(_)) => "builtin/nvidia",
        ChannelCredential::Builtin(BuiltinChannelCredential::Deepseek(_)) => "builtin/deepseek",
        ChannelCredential::Custom(_) => "custom/apikey",
    }
    .to_string()
}

pub fn credential_health_to_storage(health: &CredentialHealth) -> (String, Option<String>) {
    match health {
        CredentialHealth::Healthy => ("healthy".to_string(), None),
        CredentialHealth::Dead => ("dead".to_string(), None),
        CredentialHealth::Partial { models } => {
            ("partial".to_string(), serde_json::to_string(models).ok())
        }
    }
}

pub fn credential_health_from_storage(
    kind: &str,
    health_json: Option<&serde_json::Value>,
) -> Result<CredentialHealth, serde_json::Error> {
    match kind {
        "healthy" => Ok(CredentialHealth::Healthy),
        "dead" => Ok(CredentialHealth::Dead),
        "partial" => {
            let models = if let Some(value) = health_json {
                parse_partial_health_models(value)?
            } else {
                Vec::new()
            };
            Ok(CredentialHealth::Partial { models })
        }
        _ => Ok(CredentialHealth::Healthy),
    }
}

fn parse_partial_health_models(
    value: &serde_json::Value,
) -> Result<Vec<ModelCooldown>, serde_json::Error> {
    serde_json::from_value::<Vec<ModelCooldown>>(value.clone()).or_else(|array_err| {
        value
            .get("models")
            .cloned()
            .map(serde_json::from_value::<Vec<ModelCooldown>>)
            .unwrap_or(Err(array_err))
    })
}
