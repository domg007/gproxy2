import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "anthropic",
  "kind": "builtin/anthropic",
  "wrapper": "Builtin",
  "builtinVariant": "Anthropic",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
