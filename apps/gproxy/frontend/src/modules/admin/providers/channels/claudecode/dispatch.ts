import type { TemplateRoute } from "../../types";

export const DISPATCH_TEMPLATE_ROUTES: readonly TemplateRoute[] = [
  ["ModelList", "Claude", "ModelList", "Claude"],
  ["ModelList", "OpenAi", "ModelList", "Claude"],
  ["ModelList", "Gemini", "ModelList", "Claude"],
  ["ModelGet", "Claude", "ModelGet", "Claude"],
  ["ModelGet", "OpenAi", "ModelGet", "Claude"],
  ["ModelGet", "Gemini", "ModelGet", "Claude"],
  ["CountToken", "Claude", "CountToken", "Claude", "local"],
  ["CountToken", "OpenAi", "CountToken", "OpenAi", "local"],
  ["CountToken", "Gemini", "CountToken", "Gemini", "local"],
  ["GenerateContent", "Claude", "GenerateContent", "Claude"],
  ["GenerateContent", "OpenAi", "GenerateContent", "Claude"],
  ["GenerateContent", "OpenAiChatCompletion", "GenerateContent", "Claude"],
  ["GenerateContent", "Gemini", "GenerateContent", "Claude"],
  ["StreamGenerateContent", "Claude", "StreamGenerateContent", "Claude"],
  ["StreamGenerateContent", "OpenAi", "StreamGenerateContent", "Claude"],
  ["StreamGenerateContent", "OpenAiChatCompletion", "StreamGenerateContent", "Claude"],
  ["StreamGenerateContent", "Gemini", "StreamGenerateContent", "Claude"],
  ["StreamGenerateContent", "GeminiNDJson", "StreamGenerateContent", "Claude"],
  ["Compact", "OpenAi", "GenerateContent", "Claude"],
];
