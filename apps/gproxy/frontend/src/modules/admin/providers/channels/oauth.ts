export type ChannelOAuthQueryDefaults = Record<string, string | null | undefined>;

export type ChannelOAuthStartButton = {
  labelKey: string;
  mode?: string;
  queryDefaults?: ChannelOAuthQueryDefaults;
  fields?: readonly string[];
};

export type ChannelOAuthCallbackButton = {
  labelKey: string;
  mode?: string;
  queryDefaults?: ChannelOAuthQueryDefaults;
  fields?: readonly string[];
};

export type ChannelOAuthUi = {
  startFields: readonly string[];
  callbackFields: readonly string[];
  startDefaults?: Record<string, string>;
  callbackDefaults?: Record<string, string>;
  startButtons?: readonly ChannelOAuthStartButton[];
  callbackButtons?: readonly ChannelOAuthCallbackButton[];
};
