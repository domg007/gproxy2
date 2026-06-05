use serde::{Deserialize, Serialize};

extensible_string_enum!(EmbeddingEncodingFormat, EmbeddingEncodingFormatKnown {
    Float => "float",
    Base64 => "base64",
});

extensible_string_enum!(ReasoningEffort, ReasoningEffortKnown {
    None => "none",
    Minimal => "minimal",
    Low => "low",
    Medium => "medium",
    High => "high",
    XHigh => "xhigh",
});

extensible_string_enum!(ReasoningSummary, ReasoningSummaryKnown {
    Auto => "auto",
    Concise => "concise",
    Detailed => "detailed",
});

extensible_string_enum!(ServiceTier, ServiceTierKnown {
    Auto => "auto",
    Default => "default",
    Flex => "flex",
    Scale => "scale",
    Priority => "priority",
});

extensible_string_enum!(TruncationStrategy, TruncationStrategyKnown {
    Auto => "auto",
    Disabled => "disabled",
});

extensible_string_enum!(Verbosity, VerbosityKnown {
    Low => "low",
    Medium => "medium",
    High => "high",
});

extensible_string_enum!(PromptCacheRetention, PromptCacheRetentionKnown {
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

extensible_string_enum!(SearchContextSize, SearchContextSizeKnown {
    Low => "low",
    Medium => "medium",
    High => "high",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApproximateLocationType {
    #[serde(rename = "approximate")]
    Approximate,
}

extensible_string_enum!(TextOrAudioModality, TextOrAudioModalityKnown {
    Text => "text",
    Audio => "audio",
});

extensible_string_enum!(InputAudioFormat, InputAudioFormatKnown {
    Wav => "wav",
    Mp3 => "mp3",
});

extensible_string_enum!(AudioResponseFormat, AudioResponseFormatKnown {
    Wav => "wav",
    Aac => "aac",
    Mp3 => "mp3",
    Flac => "flac",
    Opus => "opus",
    Pcm16 => "pcm16",
});

extensible_string_enum!(DetailLevel, DetailLevelKnown {
    Auto => "auto",
    Low => "low",
    High => "high",
    Original => "original",
});
