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
    #[serde(skip_serializing_if = "Option::is_none", alias = "prelude_txt")]
    pub prelude_text: Option<ClaudeCodePreludeText>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeCodePreludeText {
    #[default]
    ClaudeCodeSystem,
    ClaudeAgentSdk,
}

impl ClaudeCodePreludeText {
    pub fn parse_loose(value: &str) -> Self {
        let value = value.trim();
        if value.is_empty() {
            return Self::ClaudeCodeSystem;
        }
        if value.eq_ignore_ascii_case("claude_agent_sdk")
            || value.eq_ignore_ascii_case("claude_agent")
            || value.eq_ignore_ascii_case("agent_sdk")
            || value == "You are a Claude agent, built on Anthropic's Claude Agent SDK."
        {
            return Self::ClaudeAgentSdk;
        }
        if value.eq_ignore_ascii_case("claude_code_system")
            || value.eq_ignore_ascii_case("claude_code")
            || value.eq_ignore_ascii_case("code_system")
            || value == "You are Claude Code, Anthropic's official CLI for Claude."
        {
            return Self::ClaudeCodeSystem;
        }
        Self::ClaudeCodeSystem
    }
}

impl<'de> Deserialize<'de> for ClaudeCodePreludeText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse_loose(&value))
    }
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

#[cfg(test)]
mod tests {
    use super::ClaudeCodePreludeText;

    #[test]
    fn claudecode_prelude_text_parses_canonical_values() {
        assert_eq!(
            serde_json::from_str::<ClaudeCodePreludeText>("\"claude_code_system\"").unwrap(),
            ClaudeCodePreludeText::ClaudeCodeSystem
        );
        assert_eq!(
            serde_json::from_str::<ClaudeCodePreludeText>("\"claude_agent_sdk\"").unwrap(),
            ClaudeCodePreludeText::ClaudeAgentSdk
        );
    }

    #[test]
    fn claudecode_prelude_text_parses_legacy_full_sentences() {
        assert_eq!(
            serde_json::from_str::<ClaudeCodePreludeText>(
                "\"You are Claude Code, Anthropic's official CLI for Claude.\""
            )
            .unwrap(),
            ClaudeCodePreludeText::ClaudeCodeSystem
        );
        assert_eq!(
            serde_json::from_str::<ClaudeCodePreludeText>(
                "\"You are a Claude agent, built on Anthropic's Claude Agent SDK.\""
            )
            .unwrap(),
            ClaudeCodePreludeText::ClaudeAgentSdk
        );
    }
}
