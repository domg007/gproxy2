import { CHANNEL_ID } from "./constants";
import { CREDENTIAL_SCHEMA } from "./credential";
import { DISPATCH_TEMPLATE_ROUTES } from "./dispatch";
import { OAUTH_UI } from "./oauth";
import { buildSettingsJson, defaultSettingsDraft, parseSettingsDraft } from "./settings";

export const CHANNEL_CONFIG = {
  channel: CHANNEL_ID,
  defaultSettingsDraft,
  parseSettingsDraft,
  buildSettingsJson,
  oauthUi: OAUTH_UI,
  credentialSchema: CREDENTIAL_SCHEMA,
  dispatchTemplateRoutes: DISPATCH_TEMPLATE_ROUTES
} as const;
