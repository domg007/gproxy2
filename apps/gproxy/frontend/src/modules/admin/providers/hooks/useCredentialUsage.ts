import { useCallback, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import { scopeAll, scopeEq } from "../../../../lib/scope";
import type { ProviderQueryRow } from "../../../../lib/types";
import {
  buildUsageDisplayRows,
  buildUsageWindowSpecs,
  parseLiveUsageRows,
  type LiveUsageRow,
  type UsageDisplayKind,
  type UsageDisplayRow,
  type UsageSampleRow
} from "../usage";
import { usagePayloadToText } from "../index";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export function useCredentialUsage({
  apiKey,
  notify,
  t,
  selectedProvider,
  providerRouteKey
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
  selectedProvider: ProviderQueryRow | null;
  providerRouteKey: string;
}) {
  const [usageByCredential, setUsageByCredential] = useState<Record<number, string>>({});
  const [liveUsageRowsByCredential, setLiveUsageRowsByCredential] = useState<
    Record<number, LiveUsageRow[]>
  >({});
  const [usageDisplayKindByCredential, setUsageDisplayKindByCredential] = useState<
    Record<number, UsageDisplayKind>
  >({});
  const [usageDisplayRowsByCredential, setUsageDisplayRowsByCredential] = useState<
    Record<number, UsageDisplayRow[]>
  >({});
  const [usageLoadingByCredential, setUsageLoadingByCredential] = useState<Record<number, boolean>>(
    {}
  );
  const [usageErrorByCredential, setUsageErrorByCredential] = useState<Record<number, string>>({});

  const clearUsageForCredential = useCallback((credentialId: number) => {
    setUsageByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    setLiveUsageRowsByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    setUsageDisplayKindByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    setUsageDisplayRowsByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    setUsageLoadingByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
    setUsageErrorByCredential((prev) => {
      const next = { ...prev };
      delete next[credentialId];
      return next;
    });
  }, []);

  const resetUsageState = useCallback(() => {
    setUsageByCredential({});
    setLiveUsageRowsByCredential({});
    setUsageDisplayKindByCredential({});
    setUsageDisplayRowsByCredential({});
    setUsageLoadingByCredential({});
    setUsageErrorByCredential({});
  }, []);

  const queryUpstreamUsage = useCallback(
    async (credentialId: number) => {
      if (!selectedProvider) {
        notify("error", t("providers.needProvider"));
        return;
      }
      setUsageLoadingByCredential((prev) => ({ ...prev, [credentialId]: true }));
      setUsageErrorByCredential((prev) => {
        const next = { ...prev };
        delete next[credentialId];
        return next;
      });

      try {
        const path = `/${providerRouteKey}/v1/usage?credential_id=${encodeURIComponent(String(credentialId))}`;
        const payload = await apiRequest<unknown>(path, {
          apiKey,
          method: "GET"
        });
        const nowMs = Date.now();
        const liveRows = parseLiveUsageRows(selectedProvider.channel, payload);
        const specs = buildUsageWindowSpecs(selectedProvider.channel, payload, liveRows, nowMs);
        let usageRows: UsageSampleRow[] = [];

        if (specs.length > 0) {
          const minFromUnixMs = specs.reduce(
            (min, item) => Math.min(min, item.fromUnixMs),
            Number.MAX_SAFE_INTEGER
          );
          const maxToUnixMs = specs.reduce((max, item) => Math.max(max, item.toUnixMs), 0);
          const rows = await apiRequest<
            Array<
              UsageSampleRow & {
                credential_id: number | null;
              }
            >
          >("/admin/usages/query", {
            apiKey,
            method: "POST",
            body: {
              channel: scopeEq(selectedProvider.channel),
              model: scopeAll<string>(),
              user_id: scopeAll<number>(),
              user_key_id: scopeAll<number>(),
              from_unix_ms: minFromUnixMs,
              to_unix_ms: maxToUnixMs,
              limit: 0
            }
          });
          usageRows = rows.filter((row) => row.credential_id === credentialId);
        }

        const usageDisplay = buildUsageDisplayRows(
          selectedProvider.channel,
          liveRows,
          specs,
          usageRows
        );

        setUsageByCredential((prev) => ({
          ...prev,
          [credentialId]: usagePayloadToText(payload)
        }));
        setLiveUsageRowsByCredential((prev) => ({
          ...prev,
          [credentialId]: liveRows
        }));
        setUsageDisplayKindByCredential((prev) => ({
          ...prev,
          [credentialId]: usageDisplay.kind
        }));
        setUsageDisplayRowsByCredential((prev) => ({
          ...prev,
          [credentialId]: usageDisplay.rows
        }));
        notify("success", t("providers.usage.fetched", { id: credentialId }));
      } catch (error) {
        setUsageErrorByCredential((prev) => ({
          ...prev,
          [credentialId]: formatError(error)
        }));
        notify("error", formatError(error));
      } finally {
        setUsageLoadingByCredential((prev) => ({ ...prev, [credentialId]: false }));
      }
    },
    [apiKey, notify, providerRouteKey, selectedProvider, t]
  );

  return {
    usageByCredential,
    liveUsageRowsByCredential,
    usageDisplayKindByCredential,
    usageDisplayRowsByCredential,
    usageLoadingByCredential,
    usageErrorByCredential,
    queryUpstreamUsage,
    clearUsageForCredential,
    resetUsageState
  };
}
