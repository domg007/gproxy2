import { useCallback, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import type { ProviderQueryRow } from "../../../../lib/types";
import { mergeQueryString, usagePayloadToText } from "../index";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

function extractOAuthState(payload: unknown): string | undefined {
  let objectPayload: Record<string, unknown> | null = null;
  if (payload && typeof payload === "object" && !Array.isArray(payload)) {
    objectPayload = payload as Record<string, unknown>;
  } else if (typeof payload === "string") {
    const trimmed = payload.trim();
    if (trimmed) {
      try {
        const parsed = JSON.parse(trimmed);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          objectPayload = parsed as Record<string, unknown>;
        }
      } catch {
        return undefined;
      }
    }
  }
  if (!objectPayload) {
    return undefined;
  }
  const state = objectPayload.state;
  if (typeof state !== "string") {
    return undefined;
  }
  const trimmed = state.trim();
  return trimmed ? trimmed : undefined;
}

export function useCredentialOAuth({
  apiKey,
  notify,
  t,
  selectedProvider,
  providerRouteKey,
  loadProviderScopedData
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
  selectedProvider: ProviderQueryRow | null;
  providerRouteKey: string;
  loadProviderScopedData: (provider: ProviderQueryRow | null) => Promise<void>;
}) {
  const [oauthStartQueryByCredential, setOauthStartQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthCallbackQueryByCredential, setOauthCallbackQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthResultByCredential, setOauthResultByCredential] = useState<Record<number, string>>(
    {}
  );

  const resetOAuthState = useCallback(() => {
    setOauthStartQueryByCredential({});
    setOauthCallbackQueryByCredential({});
    setOauthResultByCredential({});
  }, []);

  const runCredentialOAuthStart = useCallback(
    async (
      credentialId?: number,
      mode?: string,
      queryDefaults?: Record<string, string | null | undefined>
    ) => {
      if (!selectedProvider) {
        notify("error", t("providers.needProvider"));
        return;
      }
      try {
        const key = credentialId ?? 0;
        const query = mergeQueryString(oauthStartQueryByCredential[key] ?? "", {
          ...(queryDefaults ?? {}),
          credential_id: credentialId === undefined ? undefined : String(credentialId),
          mode
        });
        const payload = await apiRequest<unknown>(`/${providerRouteKey}/v1/oauth${query}`, {
          apiKey,
          method: "GET"
        });
        const oauthState = extractOAuthState(payload);
        if (oauthState) {
          setOauthCallbackQueryByCredential((prev) => ({
            ...prev,
            [key]: mergeQueryString(prev[key] ?? "", { state: oauthState })
          }));
        }
        setOauthResultByCredential((prev) => ({
          ...prev,
          [key]: usagePayloadToText(payload)
        }));
        notify("success", t("providers.oauth.startDone"));
      } catch (error) {
        notify("error", formatError(error));
      }
    },
    [apiKey, notify, oauthStartQueryByCredential, providerRouteKey, selectedProvider, t]
  );

  const runCredentialOAuthCallback = useCallback(
    async (
      credentialId?: number,
      mode?: string,
      queryDefaults?: Record<string, string | null | undefined>
    ) => {
      if (!selectedProvider) {
        notify("error", t("providers.needProvider"));
        return;
      }
      try {
        const key = credentialId ?? 0;
        const query = mergeQueryString(oauthCallbackQueryByCredential[key] ?? "", {
          ...(queryDefaults ?? {}),
          credential_id: credentialId === undefined ? undefined : String(credentialId),
          mode
        });
        const payload = await apiRequest<unknown>(`/${providerRouteKey}/v1/oauth/callback${query}`, {
          apiKey,
          method: "GET"
        });
        setOauthResultByCredential((prev) => ({
          ...prev,
          [key]: usagePayloadToText(payload)
        }));
        notify("success", t("providers.oauth.callbackDone"));
        await loadProviderScopedData(selectedProvider);
      } catch (error) {
        notify("error", formatError(error));
      }
    },
    [
      apiKey,
      loadProviderScopedData,
      notify,
      oauthCallbackQueryByCredential,
      providerRouteKey,
      selectedProvider,
      t
    ]
  );

  return {
    oauthStartQueryByCredential,
    setOauthStartQueryByCredential,
    oauthCallbackQueryByCredential,
    setOauthCallbackQueryByCredential,
    oauthResultByCredential,
    runCredentialOAuthStart,
    runCredentialOAuthCallback,
    resetOAuthState
  };
}
