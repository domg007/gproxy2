import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: [],
  callbackFields: ["callback_code"],
  startDefaults: {},
  startButtons: [
    { labelKey: "providers.oauth.startDeviceAuth", mode: "device_auth" },
    { labelKey: "providers.oauth.startAuthorizationCode", mode: "authorization_code" }
  ],
  callbackButtons: [
    {
      labelKey: "providers.oauth.submit"
    }
  ]
};
