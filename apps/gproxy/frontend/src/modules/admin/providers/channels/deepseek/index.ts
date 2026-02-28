import { CHANNEL_ID, SUPPORTS_OAUTH, SUPPORTS_UPSTREAM_USAGE } from "./constants";
import { CREDENTIAL_SCHEMA } from "./credential";
import { DISPATCH_TEMPLATE_ROUTES } from "./dispatch";
import { buildSettingsJson, defaultSettingsDraft, parseSettingsDraft } from "./settings";

export const CHANNEL_CONFIG = {
  channel: CHANNEL_ID,
  supportsOAuth: SUPPORTS_OAUTH,
  supportsUpstreamUsage: SUPPORTS_UPSTREAM_USAGE,
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson,
  credentialSchema: CREDENTIAL_SCHEMA,
  dispatchTemplateRoutes: DISPATCH_TEMPLATE_ROUTES
} as const;
