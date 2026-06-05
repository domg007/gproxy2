use serde::{Deserialize, Serialize};

strict_string_enum!(EmbeddingEncodingFormat {
    Float => "float",
    Base64 => "base64",
});

strict_string_enum!(ReasoningEffort {
    None => "none",
    Minimal => "minimal",
    Low => "low",
    Medium => "medium",
    High => "high",
    XHigh => "xhigh",
});

strict_string_enum!(ReasoningSummary {
    Auto => "auto",
    Concise => "concise",
    Detailed => "detailed",
});

strict_string_enum!(ServiceTier {
    Auto => "auto",
    Default => "default",
    Flex => "flex",
    Scale => "scale",
    Priority => "priority",
});

strict_string_enum!(TruncationStrategy {
    Auto => "auto",
    Disabled => "disabled",
});

strict_string_enum!(Verbosity {
    Low => "low",
    Medium => "medium",
    High => "high",
});

strict_string_enum!(PromptCacheRetention {
    InMemory => "in_memory",
    TwentyFourHours => "24h",
});

extensible_string_enum!(ResponsePersonality, ResponsePersonalityKnown {
    Friendly => "friendly",
    Pragmatic => "pragmatic",
});

extensible_string_enum!(ContextManagementType, ContextManagementTypeKnown {
    Compaction => "compaction",
});

strict_string_enum!(SearchContextSize {
    Low => "low",
    Medium => "medium",
    High => "high",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApproximateLocationType {
    #[serde(rename = "approximate")]
    Approximate,
}

strict_string_enum!(TextOrAudioModality {
    Text => "text",
    Audio => "audio",
});

strict_string_enum!(InputAudioFormat {
    Wav => "wav",
    Mp3 => "mp3",
});

strict_string_enum!(AudioResponseFormat {
    Wav => "wav",
    Aac => "aac",
    Mp3 => "mp3",
    Flac => "flac",
    Opus => "opus",
    Pcm16 => "pcm16",
});

extensible_string_enum!(VoiceName, VoiceNameKnown {
    Alloy => "alloy",
    Ash => "ash",
    Ballad => "ballad",
    Coral => "coral",
    Echo => "echo",
    Fable => "fable",
    Nova => "nova",
    Onyx => "onyx",
    Sage => "sage",
    Shimmer => "shimmer",
    Verse => "verse",
    Marin => "marin",
    Cedar => "cedar",
});

strict_string_enum!(DetailLevel {
    Auto => "auto",
    Low => "low",
    High => "high",
    Original => "original",
});

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn documented_closed_content_enums_reject_unknown_strings() {
        assert!(serde_json::from_value::<ServiceTier>(json!("batch")).is_err());
        assert!(serde_json::from_value::<ReasoningEffort>(json!("extreme")).is_err());
        assert!(serde_json::from_value::<PromptCacheRetention>(json!("7d")).is_err());
        assert!(serde_json::from_value::<DetailLevel>(json!("medium")).is_err());
    }

    #[test]
    fn documented_string_fallback_enums_remain_extensible() {
        assert!(matches!(
            serde_json::from_value::<VoiceName>(json!("voice_custom"))
                .expect("voice names support custom string fallback"),
            VoiceName::Unknown(value) if value == "voice_custom"
        ));
        assert!(matches!(
            serde_json::from_value::<ResponsePersonality>(json!("laconic"))
                .expect("personality supports string fallback"),
            ResponsePersonality::Unknown(value) if value == "laconic"
        ));
    }
}
