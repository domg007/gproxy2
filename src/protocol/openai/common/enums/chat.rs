extensible_string_enum!(ChatFinishReason, ChatFinishReasonKnown {
    Stop => "stop",
    Length => "length",
    ToolCalls => "tool_calls",
    ContentFilter => "content_filter",
    FunctionCall => "function_call",
});
