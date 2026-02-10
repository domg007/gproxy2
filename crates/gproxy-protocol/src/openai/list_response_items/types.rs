use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::{
    ApplyPatchToolCall, ApplyPatchToolCallOutput, CodeInterpreterToolCall,
    ComputerCallOutputItemType, ComputerCallSafetyCheckParam, ComputerScreenshotImage,
    ComputerToolCall, FileSearchToolCall, FunctionCallItemStatus, FunctionCallOutputItemType,
    FunctionShellCall, FunctionShellCallOutput, FunctionToolCallType, ImageGenToolCall,
    InputContent, InputMessageRole, InputMessageType, LocalShellToolCall, LocalShellToolCallOutput,
    MCPApprovalRequest, MCPApprovalResponseType, MCPListTools, MCPToolCall, MessageStatus,
    OutputMessage, ToolCallOutput, WebSearchToolCall,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResponseItemListObjectType {
    #[serde(rename = "list")]
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResponseItemList {
    pub object: ResponseItemListObjectType,
    pub data: Vec<ItemResource>,
    pub first_id: String,
    pub last_id: String,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputMessageResource {
    pub id: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub r#type: Option<InputMessageType>,
    pub role: InputMessageRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<MessageStatus>,
    pub content: Vec<InputContent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FunctionToolCallResource {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: FunctionToolCallType,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FunctionCallItemStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FunctionToolCallOutputResource {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: FunctionCallOutputItemType,
    pub call_id: String,
    pub output: ToolCallOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FunctionCallItemStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ComputerToolCallOutputResource {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: ComputerCallOutputItemType,
    pub call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_safety_checks: Option<Vec<ComputerCallSafetyCheckParam>>,
    pub output: ComputerScreenshotImage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FunctionCallItemStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MCPApprovalResponseResource {
    #[serde(rename = "type")]
    pub r#type: MCPApprovalResponseType,
    pub id: String,
    pub approval_request_id: String,
    pub approve: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemResource {
    InputMessage(InputMessageResource),
    OutputMessage(OutputMessage),
    FileSearch(FileSearchToolCall),
    Computer(ComputerToolCall),
    ComputerOutput(ComputerToolCallOutputResource),
    WebSearch(WebSearchToolCall),
    Function(FunctionToolCallResource),
    FunctionOutput(FunctionToolCallOutputResource),
    ImageGen(ImageGenToolCall),
    CodeInterpreter(CodeInterpreterToolCall),
    LocalShell(LocalShellToolCall),
    LocalShellOutput(LocalShellToolCallOutput),
    FunctionShell(FunctionShellCall),
    FunctionShellOutput(FunctionShellCallOutput),
    ApplyPatch(ApplyPatchToolCall),
    ApplyPatchOutput(ApplyPatchToolCallOutput),
    MCPListTools(MCPListTools),
    MCPApprovalRequest(MCPApprovalRequest),
    MCPApprovalResponse(MCPApprovalResponseResource),
    MCPCall(MCPToolCall),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_response::types::{
        FunctionToolCallType, InputTextContent, MCPApprovalResponseType,
    };

    #[test]
    fn deserializes_response_item_list_with_input_message_resource() {
        let json = r#"
        {
          "object": "list",
          "data": [
            {
              "id": "msg_abc123",
              "type": "message",
              "role": "user",
              "content": [
                {
                  "type": "input_text",
                  "text": "Tell me a bedtime story."
                }
              ]
            }
          ],
          "first_id": "msg_abc123",
          "last_id": "msg_abc123",
          "has_more": false
        }
        "#;

        let parsed: ResponseItemList =
            serde_json::from_str(json).expect("deserialize response item list");
        assert_eq!(parsed.object, ResponseItemListObjectType::List);
        assert_eq!(parsed.data.len(), 1);
        match &parsed.data[0] {
            ItemResource::InputMessage(message) => {
                assert_eq!(message.id, "msg_abc123");
                assert_eq!(message.role, InputMessageRole::User);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn round_trips_mixed_item_resources() {
        let list = ResponseItemList {
            object: ResponseItemListObjectType::List,
            data: vec![
                ItemResource::InputMessage(InputMessageResource {
                    id: "msg_1".to_string(),
                    r#type: Some(InputMessageType::Message),
                    role: InputMessageRole::User,
                    status: Some(MessageStatus::Completed),
                    content: vec![InputContent::InputText(InputTextContent {
                        text: "hi".to_string(),
                    })],
                }),
                ItemResource::Function(FunctionToolCallResource {
                    id: "fc_1".to_string(),
                    r#type: FunctionToolCallType::FunctionCall,
                    call_id: "call_1".to_string(),
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                    status: Some(FunctionCallItemStatus::Completed),
                }),
                ItemResource::MCPApprovalResponse(MCPApprovalResponseResource {
                    r#type: MCPApprovalResponseType::MCPApprovalResponse,
                    id: "mcp_appr_1".to_string(),
                    approval_request_id: "mcp_req_1".to_string(),
                    approve: true,
                    reason: None,
                }),
            ],
            first_id: "msg_1".to_string(),
            last_id: "mcp_appr_1".to_string(),
            has_more: false,
        };

        let encoded = serde_json::to_string(&list).expect("serialize response item list");
        let decoded: ResponseItemList =
            serde_json::from_str(&encoded).expect("deserialize response item list");
        assert_eq!(decoded.data.len(), 3);
    }
}
