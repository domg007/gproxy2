import { useCallback, useState } from "react";

import { apiRequest, formatError } from "../../../../lib/api";
import { parseOptionalI64, parseRequiredI64 } from "../../../../lib/form";
import type { ProviderQueryRow } from "../../../../lib/types";
import type { StatusFormState } from "../types";

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

function createInitialStatusForm(): StatusFormState {
  return {
    id: "",
    credentialId: "",
    healthKind: "healthy",
    healthJson: "",
    checkedAtUnixMs: "",
    lastError: ""
  };
}

export function useCredentialStatus({
  apiKey,
  notify,
  t,
  selectedProvider,
  loadProviderScopedData
}: {
  apiKey: string;
  notify: NotifyFn;
  t: TranslateFn;
  selectedProvider: ProviderQueryRow | null;
  loadProviderScopedData: (provider: ProviderQueryRow | null) => Promise<void>;
}) {
  const [statusEditorCredentialId, setStatusEditorCredentialId] = useState<number | null>(null);
  const [statusForm, setStatusForm] = useState<StatusFormState>(createInitialStatusForm);

  const resetStatusEditor = useCallback(() => {
    setStatusEditorCredentialId(null);
  }, []);

  const upsertStatus = useCallback(async () => {
    if (!selectedProvider) {
      notify("error", t("providers.needProvider"));
      return;
    }
    try {
      await apiRequest("/admin/credential-statuses/upsert", {
        apiKey,
        method: "POST",
        body: {
          id: parseOptionalI64(statusForm.id),
          credential_id: parseRequiredI64(statusForm.credentialId, "credential_id"),
          channel: selectedProvider.channel,
          health_kind: statusForm.healthKind.trim(),
          health_json: statusForm.healthJson.trim() || null,
          checked_at_unix_ms: parseOptionalI64(statusForm.checkedAtUnixMs),
          last_error: statusForm.lastError.trim() || null
        }
      });
      notify("success", t("providers.status.saved"));
      await loadProviderScopedData(selectedProvider);
    } catch (error) {
      notify("error", formatError(error));
    }
  }, [apiKey, loadProviderScopedData, notify, selectedProvider, statusForm, t]);

  return {
    statusEditorCredentialId,
    setStatusEditorCredentialId,
    statusForm,
    setStatusForm,
    upsertStatus,
    resetStatusEditor
  };
}
