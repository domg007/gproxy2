import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "groq",
  "kind": "builtin/groq",
  "wrapper": "Builtin",
  "builtinVariant": "Groq",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
