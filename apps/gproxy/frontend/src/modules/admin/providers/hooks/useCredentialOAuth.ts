import { useCallback, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import type {
  CredentialQueryRow,
  CredentialStatusQueryRow,
  ProviderQueryRow
} from "../../../../lib/types";
import { mergeQueryString, usagePayloadToText } from "../index";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

function toOAuthObjectPayload(payload: unknown): Record<string, unknown> | null {
  if (payload && typeof payload === "object" && !Array.isArray(payload)) {
    return payload as Record<string, unknown>;
  }
  if (typeof payload !== "string") {
    return null;
  }
  const trimmed = payload.trim();
  if (!trimmed) {
    return null;
  }
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    return null;
  }
  return null;
}

function pickOAuthString(
  payload: Record<string, unknown> | null,
  key: string
): string | undefined {
  if (!payload) {
    return undefined;
  }
  const value = payload[key];
  if (typeof value !== "string") {
    return undefined;
  }
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}

function extractOAuthState(payload: unknown): string | undefined {
  const objectPayload = toOAuthObjectPayload(payload);
  return pickOAuthString(objectPayload, "state");
}

function extractOAuthCallbackCode(payload: unknown): string | undefined {
  const objectPayload = toOAuthObjectPayload(payload);
  return (
    pickOAuthString(objectPayload, "user_code") ??
    pickOAuthString(objectPayload, "code")
  );
}

function remapCallbackQueryForRequest(rawQuery: string): string {
  const input = rawQuery.trim();
  const params = new URLSearchParams(input.startsWith("?") ? input.slice(1) : input);
  const callbackCode = params.get("callback_code")?.trim() ?? "";
  if (callbackCode) {
    params.set("code", callbackCode);
  }
  params.delete("callback_code");
  const query = params.toString();
  return query ? `?${query}` : "";
}

function normalizeCallbackDefaults(
  defaults?: Record<string, string | null | undefined>
): Record<string, string | null | undefined> {
  if (!defaults) {
    return {};
  }
  const next: Record<string, string | null | undefined> = { ...defaults };
  const callbackCode = next.callback_code;
  if (callbackCode !== undefined) {
    next.code = callbackCode;
    delete next.callback_code;
  }
  return next;
}

function buildCallbackRequestQuery(
  rawQuery: string,
  extras: Record<string, string | null | undefined>
): string {
  const normalizedBase = remapCallbackQueryForRequest(rawQuery);
  const normalizedExtras = normalizeCallbackDefaults(extras);
  return mergeQueryString(normalizedBase, normalizedExtras);
}

export function useCredentialOAuth({
  apiKey,
  notify,
  t,
  selectedProvider,
  providerRouteKey,
  loadProviderScopedData,
  refreshProviderScopedData
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
  selectedProvider: ProviderQueryRow | null;
  providerRouteKey: string;
  loadProviderScopedData: (provider: ProviderQueryRow | null) => Promise<{
    credentials: CredentialQueryRow[];
    statuses: CredentialStatusQueryRow[];
  }>;
  refreshProviderScopedData?: () => Promise<void>;
}) {
  const [oauthStartQueryByCredential, setOauthStartQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthCallbackQueryByCredential, setOauthCallbackQueryByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthActiveModeByCredential, setOauthActiveModeByCredential] = useState<
    Record<number, string>
  >({});
  const [oauthResultByCredential, setOauthResultByCredential] = useState<Record<number, string>>(
    {}
  );

  const resetOAuthState = useCallback(() => {
    setOauthStartQueryByCredential({});
    setOauthCallbackQueryByCredential({});
    setOauthActiveModeByCredential({});
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
        setOauthActiveModeByCredential((prev) => {
          if (!mode) {
            if (!(key in prev)) {
              return prev;
            }
            const next = { ...prev };
            delete next[key];
            return next;
          }
          return {
            ...prev,
            [key]: mode
          };
        });
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
        const oauthCode = extractOAuthCallbackCode(payload);
        if (oauthState || oauthCode) {
          setOauthCallbackQueryByCredential((prev) => ({
            ...prev,
            [key]: buildCallbackRequestQuery(prev[key] ?? "", {
              state: oauthState,
              code: oauthCode
            })
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
        const query = buildCallbackRequestQuery(oauthCallbackQueryByCredential[key] ?? "", {
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
        if (refreshProviderScopedData) {
          await refreshProviderScopedData();
        } else {
          await loadProviderScopedData(selectedProvider);
        }
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
      refreshProviderScopedData,
      selectedProvider,
      t
    ]
  );

  return {
    oauthStartQueryByCredential,
    setOauthStartQueryByCredential,
    oauthCallbackQueryByCredential,
    setOauthCallbackQueryByCredential,
    oauthActiveModeByCredential,
    oauthResultByCredential,
    runCredentialOAuthStart,
    runCredentialOAuthCallback,
    resetOAuthState
  };
}
