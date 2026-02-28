import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: ["redirect_uri"],
  callbackFields: [],
  startDefaults: {
    redirect_uri: "https://codeassist.google.com/authcode"
  },
  startButtons: [
    {
      labelKey: "providers.oauth.startUserCode",
      mode: "user_code",
      queryDefaults: {
        redirect_uri: "https://codeassist.google.com/authcode"
      }
    },
    {
      labelKey: "providers.oauth.startAuthorizationCode",
      mode: "authorization_code",
      queryDefaults: {
        redirect_uri: "http://127.0.0.1:1455/oauth2callback"
      }
    }
  ],
  callbackButtons: [
    {
      labelKey: "providers.oauth.callbackUserCode",
      mode: "user_code",
      fields: ["state", "user_code"],
      queryDefaults: {
        callback_url: null,
        code: null
      }
    },
    {
      labelKey: "providers.oauth.callbackAuthorizationCode",
      mode: "authorization_code",
      fields: ["callback_url", "state", "code"],
      queryDefaults: {
        user_code: null
      }
    }
  ]
};
