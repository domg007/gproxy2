use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::common::*;
use super::response_tools::ResponseTool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseItem {
    Message(ResponseMessageItem),
    Typed(TypedResponseItem),
    Unknown(UnknownResponseItem),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseMessageItem {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<ResponseMessageItemType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: ResponseMessageRole,
    pub content: ResponseContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResponseItemStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ResponsePhase>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseMessageItemType {
    #[serde(rename = "message")]
    Message,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TypedResponseItem {
    #[serde(rename = "file_search_call")]
    FileSearchCall {
        id: String,
        queries: Vec<String>,
        status: ResponseItemStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        results: Option<Vec<FileSearchResult>>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "computer_call")]
    ComputerCall {
        id: String,
        call_id: String,
        pending_safety_checks: Vec<SafetyCheck>,
        status: ResponseItemStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<ComputerAction>,
        #[serde(skip_serializing_if = "Option::is_none")]
        actions: Option<Vec<ComputerAction>>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "computer_call_output")]
    ComputerCallOutput {
        call_id: String,
        output: ComputerScreenshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        acknowledged_safety_checks: Option<Vec<SafetyCheck>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "web_search_call")]
    WebSearchCall {
        id: String,
        action: WebSearchAction,
        status: ResponseItemStatus,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        arguments: String,
        call_id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "function_call_output")]
    FunctionCallOutput {
        call_id: String,
        output: ResponseOutput,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "tool_search_call")]
    ToolSearchCall {
        arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        execution: Option<ToolSearchExecution>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "tool_search_output")]
    ToolSearchOutput {
        tools: Vec<ResponseTool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "additional_tools")]
    AdditionalTools {
        role: AdditionalToolsRole,
        tools: Vec<ResponseTool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        id: String,
        summary: Vec<ResponseReasoningSummaryPart>,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ResponseReasoningTextPart>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "compaction")]
    Compaction {
        encrypted_content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_generation_call")]
    ImageGenerationCall {
        id: String,
        result: String,
        status: ResponseItemStatus,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "code_interpreter_call")]
    CodeInterpreterCall {
        id: String,
        code: Option<String>,
        container_id: String,
        outputs: Option<Vec<CodeInterpreterOutput>>,
        status: ResponseItemStatus,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "local_shell_call")]
    LocalShellCall {
        id: String,
        action: LocalShellAction,
        call_id: String,
        status: ResponseItemStatus,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "local_shell_call_output")]
    LocalShellCallOutput {
        id: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "shell_call")]
    ShellCall {
        action: ShellAction,
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        environment: Option<ShellEnvironment>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "shell_call_output")]
    ShellCallOutput {
        call_id: String,
        output: Vec<ShellCallOutputContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_output_length: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "apply_patch_call")]
    ApplyPatchCall {
        call_id: String,
        operation: ApplyPatchOperation,
        status: ResponseItemStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "apply_patch_call_output")]
    ApplyPatchCallOutput {
        call_id: String,
        status: ResponseItemStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "mcp_list_tools")]
    McpListTools {
        id: String,
        server_label: String,
        tools: Vec<McpToolDescription>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "mcp_approval_request")]
    McpApprovalRequest {
        id: String,
        arguments: String,
        name: String,
        server_label: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "mcp_approval_response")]
    McpApprovalResponse {
        approval_request_id: String,
        approve: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "mcp_call")]
    McpCall {
        id: String,
        arguments: String,
        name: String,
        server_label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        approval_request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<ResponseItemStatus>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "custom_tool_call")]
    CustomToolCall {
        call_id: String,
        input: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "custom_tool_call_output")]
    CustomToolCallOutput {
        call_id: String,
        output: ResponseOutput,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "compaction_trigger")]
    CompactionTrigger {
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "item_reference")]
    ItemReference {
        id: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownResponseItem {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<ResponseItemType>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseContent {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseOutput {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseContentPart {
    #[serde(rename = "input_text")]
    InputText {
        text: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "input_image")]
    InputImage {
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<DetailLevel>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        image_url: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "input_file")]
    InputFile {
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<DetailLevel>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "input_audio")]
    InputAudio {
        input_audio: InputAudioContent,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "output_text")]
    OutputText {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        annotations: Vec<ResponseAnnotation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<TokenLogprob>>,
        text: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "refusal")]
    Refusal {
        refusal: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputAudioContent {
    pub data: String,
    pub format: InputAudioFormat,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseAnnotation {
    #[serde(rename = "file_citation")]
    FileCitation {
        file_id: String,
        filename: String,
        index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "url_citation")]
    UrlCitation {
        end_index: u32,
        start_index: u32,
        title: String,
        url: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "container_file_citation")]
    ContainerFileCitation {
        container_id: String,
        end_index: u32,
        file_id: String,
        filename: String,
        start_index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "file_path")]
    FilePath {
        file_id: String,
        index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSearchResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyCheck {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ComputerAction {
    #[serde(rename = "click")]
    Click {
        button: ComputerMouseButton,
        x: f64,
        y: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "double_click")]
    DoubleClick { keys: Vec<String>, x: f64, y: f64 },
    #[serde(rename = "drag")]
    Drag {
        path: Vec<ComputerCoordinate>,
        #[serde(skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "keypress")]
    Keypress { keys: Vec<String> },
    #[serde(rename = "move")]
    Move {
        x: f64,
        y: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "screenshot")]
    Screenshot {},
    #[serde(rename = "scroll")]
    Scroll {
        scroll_x: f64,
        scroll_y: f64,
        x: f64,
        y: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
    },
    #[serde(rename = "type")]
    Type { text: String },
    #[serde(rename = "wait")]
    Wait {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputerMouseButton {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "wheel")]
    Wheel,
    #[serde(rename = "back")]
    Back,
    #[serde(rename = "forward")]
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerCoordinate {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerScreenshot {
    #[serde(rename = "type")]
    pub type_: ComputerScreenshotType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputerScreenshotType {
    #[serde(rename = "computer_screenshot")]
    ComputerScreenshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSearchAction {
    #[serde(rename = "search")]
    Search {
        #[serde(skip_serializing_if = "Option::is_none")]
        queries: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sources: Option<Vec<WebSearchSource>>,
    },
    #[serde(rename = "open_page")]
    OpenPage {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    #[serde(rename = "find_in_page")]
    FindInPage { pattern: String, url: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchSource {
    #[serde(rename = "type")]
    pub type_: WebSearchSourceType,
    pub url: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebSearchSourceType {
    #[serde(rename = "url")]
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdditionalToolsRole {
    #[serde(rename = "developer")]
    Developer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseReasoningSummaryPart {
    pub text: String,
    #[serde(rename = "type")]
    pub type_: ResponseReasoningSummaryType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseReasoningSummaryType {
    #[serde(rename = "summary_text")]
    SummaryText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseReasoningTextPart {
    pub text: String,
    #[serde(rename = "type")]
    pub type_: ResponseReasoningTextType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseReasoningTextType {
    #[serde(rename = "reasoning_text")]
    ReasoningText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CodeInterpreterOutput {
    #[serde(rename = "logs")]
    Logs { logs: String },
    #[serde(rename = "image")]
    Image { url: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalShellAction {
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    #[serde(rename = "type")]
    pub type_: LocalShellActionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalShellActionType {
    #[serde(rename = "exec")]
    Exec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellAction {
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShellEnvironment {
    #[serde(rename = "local")]
    Local {
        #[serde(skip_serializing_if = "Option::is_none")]
        skills: Option<Vec<ShellSkillReference>>,
    },
    #[serde(rename = "container_reference")]
    ContainerReference { container_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellSkillReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellCallOutputContent {
    pub outcome: ShellCallOutcome,
    pub stderr: String,
    pub stdout: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShellCallOutcome {
    #[serde(rename = "timeout")]
    Timeout {},
    #[serde(rename = "exit")]
    Exit { exit_code: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ApplyPatchOperation {
    #[serde(rename = "create_file")]
    CreateFile { diff: String, path: String },
    #[serde(rename = "delete_file")]
    DeleteFile { path: String },
    #[serde(rename = "update_file")]
    UpdateFile { diff: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolDescription {
    pub input_schema: Value,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn response_item_accepts_easy_message_without_type() {
        let item: ResponseItem = serde_json::from_value(json!({
            "role": "user",
            "content": [
                { "type": "input_text", "text": "hello" }
            ]
        }))
        .expect("easy message should deserialize");

        assert!(matches!(item, ResponseItem::Message(_)));
    }

    #[test]
    fn response_item_models_function_call_output_content() {
        let item: ResponseItem = serde_json::from_value(json!({
            "type": "function_call_output",
            "call_id": "call_123",
            "output": [
                { "type": "input_text", "text": "{\"ok\":true}" }
            ],
            "status": "completed"
        }))
        .expect("function call output should deserialize");

        let ResponseItem::Typed(TypedResponseItem::FunctionCallOutput { output, .. }) = item else {
            panic!("expected function_call_output item");
        };
        assert!(matches!(output, ResponseOutput::Parts(_)));
    }

    #[test]
    fn response_item_models_web_search_action() {
        let item: ResponseItem = serde_json::from_value(json!({
            "type": "web_search_call",
            "id": "ws_123",
            "status": "searching",
            "action": {
                "type": "search",
                "query": "openai api docs",
                "sources": [{ "type": "url", "url": "https://example.com" }]
            }
        }))
        .expect("web search call should deserialize");

        let ResponseItem::Typed(TypedResponseItem::WebSearchCall { action, .. }) = item else {
            panic!("expected web_search_call item");
        };
        assert!(matches!(action, WebSearchAction::Search { .. }));
    }

    #[test]
    fn response_item_keeps_unknown_item_type_extensible() {
        let item: ResponseItem = serde_json::from_value(json!({
            "type": "future_item",
            "payload": { "x": 1 }
        }))
        .expect("unknown item should deserialize");

        assert!(matches!(item, ResponseItem::Unknown(_)));
    }
}
