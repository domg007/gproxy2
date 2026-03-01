import type { TemplateRoute } from "../../types";

export const DISPATCH_TEMPLATE_ROUTES: readonly TemplateRoute[] = [
  ["ModelList", "OpenAi", "ModelList", "OpenAi"],
  ["ModelList", "Claude", "ModelList", "OpenAi"],
  ["ModelList", "Gemini", "ModelList", "OpenAi"],
  ["ModelGet", "OpenAi", "ModelGet", "OpenAi"],
  ["ModelGet", "Claude", "ModelGet", "OpenAi"],
  ["ModelGet", "Gemini", "ModelGet", "OpenAi"],
  ["CountToken", "OpenAi", "CountToken", "OpenAi", "local"],
  ["CountToken", "Claude", "CountToken", "OpenAi", "local"],
  ["CountToken", "Gemini", "CountToken", "OpenAi", "local"],
  ["GenerateContent", "OpenAi", "GenerateContent", "OpenAi"],
  ["GenerateContent", "OpenAiChatCompletion", "GenerateContent", "OpenAiChatCompletion"],
  ["GenerateContent", "Claude", "GenerateContent", "OpenAi"],
  ["GenerateContent", "Gemini", "GenerateContent", "OpenAi"],
  ["StreamGenerateContent", "OpenAi", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "OpenAiChatCompletion", "StreamGenerateContent", "OpenAiChatCompletion"],
  ["StreamGenerateContent", "Claude", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "Gemini", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "GeminiNDJson", "StreamGenerateContent", "OpenAi"],
  ["Compact", "OpenAi", "GenerateContent", "OpenAi"],
];
