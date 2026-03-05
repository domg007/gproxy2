import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: ["redirect_uri"],
  callbackFields: ["callback_url", "callback_code"],
  startDefaults: {
    redirect_uri: "http://localhost:51121/oauth-callback"
  }
};
