import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: [],
  callbackFields: [],
  startDefaults: {},
  startButtons: [
    { labelKey: "providers.oauth.startDeviceAuth", mode: "device_auth" },
    { labelKey: "providers.oauth.startAuthorizationCode", mode: "authorization_code" }
  ],
  callbackButtons: [
    {
      labelKey: "providers.oauth.callbackDeviceAuth",
      mode: "device_auth",
      fields: [],
      queryDefaults: {
        callback_url: null,
        code: null
      }
    },
    {
      labelKey: "providers.oauth.callbackAuthorizationCode",
      mode: "authorization_code",
      fields: ["callback_url", "state", "code"]
    }
  ]
};
