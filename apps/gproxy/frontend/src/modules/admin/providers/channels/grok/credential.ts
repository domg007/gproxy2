import type { ChannelCredentialSchema } from "../../types";

export const CREDENTIAL_SCHEMA: ChannelCredentialSchema = {
  channel: "grok-web",
  kind: "builtin/grok-web",
  wrapper: "Builtin",
  builtinVariant: "Grok",
  fields: [
    {
      key: "sso",
      label: "sso",
      type: "string"
    }
  ]
};
