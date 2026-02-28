import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "aistudio",
  "kind": "builtin/aistudio",
  "wrapper": "Builtin",
  "builtinVariant": "AiStudio",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
