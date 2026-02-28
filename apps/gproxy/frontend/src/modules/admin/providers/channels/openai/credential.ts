import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "openai",
  "kind": "builtin/openai",
  "wrapper": "Builtin",
  "builtinVariant": "OpenAi",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
