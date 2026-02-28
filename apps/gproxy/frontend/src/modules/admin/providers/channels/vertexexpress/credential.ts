import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "vertexexpress",
  "kind": "builtin/vertexexpress",
  "wrapper": "Builtin",
  "builtinVariant": "VertexExpress",
  "fields": [
    {
      "key": "api_key",
      "label": "api_key",
      "type": "string"
    }
  ]
};
