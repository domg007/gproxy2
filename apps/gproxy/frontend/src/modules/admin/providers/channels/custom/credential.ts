import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "custom",
  "kind": "custom/apikey",
  "wrapper": "Custom",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
