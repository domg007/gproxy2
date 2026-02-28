import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "antigravity",
  "kind": "builtin/antigravity",
  "wrapper": "Builtin",
  "builtinVariant": "Antigravity",
  "fields": [
    {
      "key": "access_token",
      "label": "access_token",
      "type": "string"
    },
    {
      "key": "refresh_token",
      "label": "refresh_token",
      "type": "string"
    },
    {
      "key": "expires_at",
      "label": "expires_at",
      "type": "integer"
    },
    {
      "key": "client_id",
      "label": "client_id",
      "type": "string"
    },
    {
      "key": "client_secret",
      "label": "client_secret",
      "type": "string"
    },
    {
      "key": "user_email",
      "label": "user_email",
      "type": "optional_string"
    }
  ]
};
