use serde::{Deserialize, Serialize};

use crate::Proto;

use super::{DispatchTable, ModelTable};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channel_settings", rename_all = "lowercase")]
pub enum ProviderConfig {
    OpenAI(OpenAIConfig),
    Claude(ClaudeConfig),
    AIStudio(AIStudioConfig),
    VertexExpress(VertexExpressConfig),
    Vertex(VertexConfig),
    GeminiCli(GeminiCliConfig),
    ClaudeCode(ClaudeCodeConfig),
    Codex(CodexConfig),
    Antigravity(AntigravityConfig),
    Nvidia(NvidiaConfig),
    DeepSeek(DeepSeekConfig),
    Custom(CustomProviderConfig),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAIConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AIStudioConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VertexExpressConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VertexConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_token_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeminiCliConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeCodeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_ai_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "console_base_url")]
    pub platform_base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NvidiaConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hf_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hf_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeepSeekConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    pub id: String,
    pub enabled: bool,
    pub proto: Proto,
    pub base_url: String,
    pub dispatch: DispatchTable,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_table: Option<ModelTable>,
    #[serde(default)]
    pub count_tokens: CountTokensMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub json_param_mask: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CountTokensMode {
    #[default]
    Upstream,
    Tokenizers,
    Tiktoken,
}
