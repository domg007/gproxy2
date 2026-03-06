import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: [],
  callbackFields: [],
  startDefaults: {},
  startButtons: [
    { labelKey: "providers.oauth.startAuthorizationCode", mode: "authorization_code" },
    { labelKey: "providers.oauth.startDeviceAuth", mode: "device_auth" }
  ],
  callbackButtons: [
    {
      labelKey: "providers.oauth.callbackAuthorizationCode",
      mode: "authorization_code",
      fields: ["callback_url"],
      queryDefaults: {
        callback_code: null,
        code: null
      }
    },
    {
      labelKey: "providers.oauth.callbackDeviceAuth",
      mode: "device_auth",
      fields: ["callback_code"],
      queryDefaults: {
        callback_url: null,
        callback_code: null,
        code: null
      }
    }
  ]
};
