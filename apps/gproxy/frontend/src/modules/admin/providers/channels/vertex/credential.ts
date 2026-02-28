import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  "channel": "vertex",
  "kind": "builtin/vertex",
  "wrapper": "Builtin",
  "builtinVariant": "Vertex",
  "fields": [
    {
      "key": "project_id",
      "label": "project_id",
      "type": "string"
    },
    {
      "key": "client_email",
      "label": "client_email",
      "type": "string"
    },
    {
      "key": "private_key",
      "label": "private_key",
      "type": "string"
    },
    {
      "key": "private_key_id",
      "label": "private_key_id",
      "type": "string"
    },
    {
      "key": "client_id",
      "label": "client_id",
      "type": "string"
    },
    {
      "key": "auth_uri",
      "label": "auth_uri",
      "type": "optional_string"
    },
    {
      "key": "token_uri",
      "label": "token_uri",
      "type": "optional_string"
    },
    {
      "key": "auth_provider_x509_cert_url",
      "label": "auth_provider_x509_cert_url",
      "type": "optional_string"
    },
    {
      "key": "client_x509_cert_url",
      "label": "client_x509_cert_url",
      "type": "optional_string"
    },
    {
      "key": "universe_domain",
      "label": "universe_domain",
      "type": "optional_string"
    },
    {
      "key": "access_token",
      "label": "access_token",
      "type": "string"
    },
    {
      "key": "expires_at",
      "label": "expires_at",
      "type": "integer"
    }
  ]
};
