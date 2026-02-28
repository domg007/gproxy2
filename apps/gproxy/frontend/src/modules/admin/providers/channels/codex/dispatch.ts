import type { TemplateRoute } from "../../types";

export const DISPATCH_TEMPLATE_ROUTES: readonly TemplateRoute[] = [
  ["ModelList", "OpenAi", "ModelList", "OpenAi"],
  ["ModelList", "Claude", "ModelList", "OpenAi"],
  ["ModelList", "Gemini", "ModelList", "OpenAi"],
  ["ModelGet", "OpenAi", "ModelGet", "OpenAi"],
  ["ModelGet", "Claude", "ModelGet", "OpenAi"],
  ["ModelGet", "Gemini", "ModelGet", "OpenAi"],
  ["CountToken", "OpenAi", "CountToken", "OpenAi"],
  ["CountToken", "Claude", "CountToken", "OpenAi"],
  ["CountToken", "Gemini", "CountToken", "OpenAi"],
  ["StreamGenerateContent", "OpenAi", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "OpenAiChatCompletion", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "Claude", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "Gemini", "StreamGenerateContent", "OpenAi"],
  ["StreamGenerateContent", "GeminiNDJson", "StreamGenerateContent", "OpenAi"],
  ["Compact", "OpenAi", "Compact", "OpenAi"],
];
