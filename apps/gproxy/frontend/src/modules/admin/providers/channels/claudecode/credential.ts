import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "claudecode",
  "kind": "builtin/claudecode",
  "wrapper": "Builtin",
  "builtinVariant": "ClaudeCode",
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
      "key": "subscription_type",
      "label": "subscription_type",
      "type": "string"
    },
    {
      "key": "rate_limit_tier",
      "label": "rate_limit_tier",
      "type": "string"
    },
    {
      "key": "cookie",
      "label": "cookie",
      "type": "optional_string"
    },
    {
      "key": "user_email",
      "label": "user_email",
      "type": "optional_string"
    }
  ]
};
