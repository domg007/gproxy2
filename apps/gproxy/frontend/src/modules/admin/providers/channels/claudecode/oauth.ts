import type { ChannelOAuthUi } from "../oauth";

export const OAUTH_UI: ChannelOAuthUi = {
  startFields: [],
  callbackFields: ["callback_url", "callback_code"],
  startDefaults: {}
};
