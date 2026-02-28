import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "deepseek",
  "kind": "builtin/deepseek",
  "wrapper": "Builtin",
  "builtinVariant": "Deepseek",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
