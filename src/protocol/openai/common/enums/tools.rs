use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSearchExecution {
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "client")]
    Client,
}

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
