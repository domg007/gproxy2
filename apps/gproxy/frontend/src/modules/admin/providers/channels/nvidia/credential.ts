import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "nvidia",
  "kind": "builtin/nvidia",
  "wrapper": "Builtin",
  "builtinVariant": "Nvidia",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
