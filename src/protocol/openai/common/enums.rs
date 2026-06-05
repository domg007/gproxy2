use serde::{Deserialize, Serialize};

macro_rules! extensible_string_enum {
    ($outer:ident, $known:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(untagged)]
        pub enum $outer {
            Known($known),
            Unknown(String),
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $known {
            $(
                #[serde(rename = $wire)]
                $variant,
            )+
        }
    };
}

extensible_string_enum!(OpenAiObjectType, OpenAiObjectTypeKnown {
    ChatCompletion => "chat.completion",
    ChatCompletionChunk => "chat.completion.chunk",
    Response => "response",
    ResponseInputTokens => "response.input_tokens",
    List => "list",
    Model => "model",
    Embedding => "embedding",
});

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSearchExecution {
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "client")]
    Client,
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

extensible_string_enum!(ImageBackground, ImageBackgroundKnown {
    Transparent => "transparent",
    Opaque => "opaque",
    Auto => "auto",
});

extensible_string_enum!(ImageModeration, ImageModerationKnown {
    Low => "low",
    Auto => "auto",
});

extensible_string_enum!(ImageOutputFormat, ImageOutputFormatKnown {
    Png => "png",
    Jpeg => "jpeg",
    Webp => "webp",
});

extensible_string_enum!(ImageQuality, ImageQualityKnown {
    Standard => "standard",
    Hd => "hd",
    Low => "low",
    Medium => "medium",
    High => "high",
    Auto => "auto",
});

extensible_string_enum!(ImageResponseFormat, ImageResponseFormatKnown {
    Url => "url",
    B64Json => "b64_json",
});

extensible_string_enum!(ImageSize, ImageSizeKnown {
    Auto => "auto",
    Size1024By1024 => "1024x1024",
    Size1536By1024 => "1536x1024",
    Size1024By1536 => "1024x1536",
    Size256By256 => "256x256",
    Size512By512 => "512x512",
    Size1792By1024 => "1792x1024",
    Size1024By1792 => "1024x1792",
});

extensible_string_enum!(ImageStyle, ImageStyleKnown {
    Vivid => "vivid",
    Natural => "natural",
});

extensible_string_enum!(ImageInputFidelity, ImageInputFidelityKnown {
    High => "high",
    Low => "low",
});

extensible_string_enum!(ImageStreamEventType, ImageStreamEventTypeKnown {
    ImageGenerationPartialImage => "image_generation.partial_image",
    ImageGenerationCompleted => "image_generation.completed",
    ImageEditPartialImage => "image_edit.partial_image",
    ImageEditCompleted => "image_edit.completed",
});

extensible_string_enum!(ToolChoiceMode, ToolChoiceModeKnown {
    None => "none",
    Auto => "auto",
    Required => "required",
});

extensible_string_enum!(AllowedToolsMode, AllowedToolsModeKnown {
    Auto => "auto",
    Required => "required",
});

extensible_string_enum!(CustomToolInputFormatType, CustomToolInputFormatTypeKnown {
    Text => "text",
    Grammar => "grammar",
});

extensible_string_enum!(CustomToolGrammarSyntax, CustomToolGrammarSyntaxKnown {
    Lark => "lark",
    Regex => "regex",
});

extensible_string_enum!(CodeInterpreterContainerType, CodeInterpreterContainerTypeKnown {
    Auto => "auto",
});

extensible_string_enum!(CodeInterpreterMemoryLimit, CodeInterpreterMemoryLimitKnown {
    OneG => "1g",
    FourG => "4g",
    SixteenG => "16g",
    SixtyFourG => "64g",
});

extensible_string_enum!(ImageGenerationAction, ImageGenerationActionKnown {
    Generate => "generate",
    Edit => "edit",
    Auto => "auto",
});

extensible_string_enum!(ToolType, ToolTypeKnown {
    Function => "function",
    Custom => "custom",
    FileSearch => "file_search",
    WebSearchPreview => "web_search_preview",
    WebSearchPreview20250311 => "web_search_preview_2025_03_11",
    WebSearch => "web_search",
    WebSearch20250826 => "web_search_2025_08_26",
    Computer => "computer",
    ComputerUse => "computer_use",
    ComputerUsePreview => "computer_use_preview",
    CodeInterpreter => "code_interpreter",
    ImageGeneration => "image_generation",
    Mcp => "mcp",
    ApplyPatch => "apply_patch",
    Shell => "shell",
    LocalShell => "local_shell",
    ToolSearch => "tool_search",
    Namespace => "namespace",
});

extensible_string_enum!(ResponseStatus, ResponseStatusKnown {
    Completed => "completed",
    Failed => "failed",
    InProgress => "in_progress",
    Cancelled => "cancelled",
    Queued => "queued",
    Incomplete => "incomplete",
});

extensible_string_enum!(IncompleteReason, IncompleteReasonKnown {
    MaxOutputTokens => "max_output_tokens",
    ContentFilter => "content_filter",
});

extensible_string_enum!(ResponseItemStatus, ResponseItemStatusKnown {
    InProgress => "in_progress",
    Completed => "completed",
    Incomplete => "incomplete",
    Searching => "searching",
    Failed => "failed",
    Generating => "generating",
    Interpreting => "interpreting",
    Calling => "calling",
});

extensible_string_enum!(ResponsePhase, ResponsePhaseKnown {
    Commentary => "commentary",
    FinalAnswer => "final_answer",
});

extensible_string_enum!(ResponseMessageRole, ResponseMessageRoleKnown {
    User => "user",
    Assistant => "assistant",
    System => "system",
    Developer => "developer",
});

extensible_string_enum!(ChatMessageRole, ChatMessageRoleKnown {
    Developer => "developer",
    System => "system",
    User => "user",
    Assistant => "assistant",
    Tool => "tool",
    Function => "function",
});

extensible_string_enum!(ChatFinishReason, ChatFinishReasonKnown {
    Stop => "stop",
    Length => "length",
    ToolCalls => "tool_calls",
    ContentFilter => "content_filter",
    FunctionCall => "function_call",
});

extensible_string_enum!(ResponseItemType, ResponseItemTypeKnown {
    Message => "message",
    FileSearchCall => "file_search_call",
    ComputerCall => "computer_call",
    ComputerCallOutput => "computer_call_output",
    WebSearchCall => "web_search_call",
    FunctionCall => "function_call",
    FunctionCallOutput => "function_call_output",
    ToolSearchCall => "tool_search_call",
    ToolSearchOutput => "tool_search_output",
    AdditionalTools => "additional_tools",
    Reasoning => "reasoning",
    Compaction => "compaction",
    ImageGenerationCall => "image_generation_call",
    CodeInterpreterCall => "code_interpreter_call",
    LocalShellCall => "local_shell_call",
    LocalShellCallOutput => "local_shell_call_output",
    ShellCall => "shell_call",
    ShellCallOutput => "shell_call_output",
    ApplyPatchCall => "apply_patch_call",
    ApplyPatchCallOutput => "apply_patch_call_output",
    McpListTools => "mcp_list_tools",
    McpApprovalRequest => "mcp_approval_request",
    McpApprovalResponse => "mcp_approval_response",
    McpCall => "mcp_call",
    CustomToolCall => "custom_tool_call",
    CustomToolCallOutput => "custom_tool_call_output",
    CompactionTrigger => "compaction_trigger",
    ItemReference => "item_reference",
});

extensible_string_enum!(ResponseIncludable, ResponseIncludableKnown {
    FileSearchCallResults => "file_search_call.results",
    WebSearchCallResults => "web_search_call.results",
    WebSearchCallActionSources => "web_search_call.action.sources",
    MessageInputImageImageUrl => "message.input_image.image_url",
    ComputerCallOutputOutputImageUrl => "computer_call_output.output.image_url",
    CodeInterpreterCallOutputs => "code_interpreter_call.outputs",
    ReasoningEncryptedContent => "reasoning.encrypted_content",
    MessageOutputTextLogprobs => "message.output_text.logprobs",
});

extensible_string_enum!(ResponseStreamEventType, ResponseStreamEventTypeKnown {
    ResponseCreated => "response.created",
    ResponseInProgress => "response.in_progress",
    ResponseCompleted => "response.completed",
    ResponseFailed => "response.failed",
    ResponseIncomplete => "response.incomplete",
    ResponseQueued => "response.queued",
    ResponseOutputItemAdded => "response.output_item.added",
    ResponseOutputItemDone => "response.output_item.done",
    ResponseContentPartAdded => "response.content_part.added",
    ResponseContentPartDone => "response.content_part.done",
    ResponseOutputTextDelta => "response.output_text.delta",
    ResponseOutputTextDone => "response.output_text.done",
    ResponseOutputTextAnnotationAdded => "response.output_text.annotation.added",
    ResponseFunctionCallArgumentsDelta => "response.function_call_arguments.delta",
    ResponseFunctionCallArgumentsDone => "response.function_call_arguments.done",
    ResponseCustomToolCallInputDelta => "response.custom_tool_call_input.delta",
    ResponseCustomToolCallInputDone => "response.custom_tool_call_input.done",
    ResponseRefusalDelta => "response.refusal.delta",
    ResponseRefusalDone => "response.refusal.done",
    ResponseReasoningSummaryPartAdded => "response.reasoning_summary_part.added",
    ResponseReasoningSummaryPartDone => "response.reasoning_summary_part.done",
    ResponseReasoningSummaryTextDelta => "response.reasoning_summary_text.delta",
    ResponseReasoningSummaryTextDone => "response.reasoning_summary_text.done",
    ResponseReasoningTextDelta => "response.reasoning_text.delta",
    ResponseReasoningTextDone => "response.reasoning_text.done",
    ResponseAudioDelta => "response.audio.delta",
    ResponseAudioDone => "response.audio.done",
    ResponseAudioTranscriptDelta => "response.audio.transcript.delta",
    ResponseAudioTranscriptDone => "response.audio.transcript.done",
    ResponseImageGenerationCallCompleted => "response.image_generation_call.completed",
    ResponseImageGenerationCallGenerating => "response.image_generation_call.generating",
    ResponseImageGenerationCallInProgress => "response.image_generation_call.in_progress",
    ResponseImageGenerationCallPartialImage => "response.image_generation_call.partial_image",
    ResponseFileSearchCallInProgress => "response.file_search_call.in_progress",
    ResponseFileSearchCallSearching => "response.file_search_call.searching",
    ResponseFileSearchCallCompleted => "response.file_search_call.completed",
    ResponseWebSearchCallInProgress => "response.web_search_call.in_progress",
    ResponseWebSearchCallSearching => "response.web_search_call.searching",
    ResponseWebSearchCallCompleted => "response.web_search_call.completed",
    ResponseCodeInterpreterCallInProgress => "response.code_interpreter_call.in_progress",
    ResponseCodeInterpreterCallInterpreting => "response.code_interpreter_call.interpreting",
    ResponseCodeInterpreterCallCompleted => "response.code_interpreter_call.completed",
    ResponseCodeInterpreterCallCodeDelta => "response.code_interpreter_call_code.delta",
    ResponseCodeInterpreterCallCodeDone => "response.code_interpreter_call_code.done",
    ResponseMcpCallArgumentsDelta => "response.mcp_call_arguments.delta",
    ResponseMcpCallArgumentsDone => "response.mcp_call_arguments.done",
    ResponseMcpCallInProgress => "response.mcp_call.in_progress",
    ResponseMcpCallCompleted => "response.mcp_call.completed",
    ResponseMcpCallFailed => "response.mcp_call.failed",
    ResponseMcpListToolsInProgress => "response.mcp_list_tools.in_progress",
    ResponseMcpListToolsCompleted => "response.mcp_list_tools.completed",
    ResponseMcpListToolsFailed => "response.mcp_list_tools.failed",
    Error => "error",
});
