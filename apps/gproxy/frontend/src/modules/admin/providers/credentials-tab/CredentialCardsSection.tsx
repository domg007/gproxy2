import type { Dispatch, SetStateAction } from "react";

import type {
  CredentialQueryRow,
  CredentialStatusQueryRow
} from "../../../../lib/types";
import { formatAtForViewer } from "../../../../lib/datetime";
import { Button, Input, Label, TextArea } from "../../../../components/ui";
import { formatUsagePercent } from "../index";
import type { LiveUsageRow, StatusFormState, UsageDisplayKind, UsageDisplayRow } from "../index";
import type { CooldownItem, CredentialHealthKind, TranslateFn } from "./shared";

export function CredentialCardsSection({
  channel,
  credentialRows,
  statusesByCredential,
  usageByCredential,
  liveUsageRowsByCredential,
  usageDisplayKindByCredential,
  usageDisplayRowsByCredential,
  usageLoadingByCredential,
  usageErrorByCredential,
  supportsUpstreamUsage,
  expandedCooldownCredentialId,
  setExpandedCooldownCredentialId,
  selectedCooldownKeysByCredential,
  setSelectedCooldownKeysByCredential,
  statusEditorCredentialId,
  setStatusEditorCredentialId,
  statusForm,
  setStatusForm,
  onEditCredential,
  onCopyCredential,
  onRemoveCredential,
  onToggleCredentialEnabled,
  onSetCredentialHealth,
  onQueryUpstreamUsage,
  onUpsertStatus,
  normalizeHealthKind,
  parseCooldowns,
  healthLabel,
  cooldownKey,
  formatWindowLabel,
  resolveUsageGroupLabel,
  resolveLiveLimitLabel,
  t
}: {
  channel: string;
  credentialRows: CredentialQueryRow[];
  statusesByCredential: Map<number, CredentialStatusQueryRow[]>;
  usageByCredential: Record<number, string>;
  liveUsageRowsByCredential: Record<number, LiveUsageRow[]>;
  usageDisplayKindByCredential: Record<number, UsageDisplayKind>;
  usageDisplayRowsByCredential: Record<number, UsageDisplayRow[]>;
  usageLoadingByCredential: Record<number, boolean>;
  usageErrorByCredential: Record<number, string>;
  supportsUpstreamUsage: boolean;
  expandedCooldownCredentialId: number | null;
  setExpandedCooldownCredentialId: Dispatch<SetStateAction<number | null>>;
  selectedCooldownKeysByCredential: Record<number, string[]>;
  setSelectedCooldownKeysByCredential: Dispatch<SetStateAction<Record<number, string[]>>>;
  statusEditorCredentialId: number | null;
  setStatusEditorCredentialId: Dispatch<SetStateAction<number | null>>;
  statusForm: StatusFormState;
  setStatusForm: Dispatch<SetStateAction<StatusFormState>>;
  onEditCredential: (row: CredentialQueryRow) => void;
  onCopyCredential: (row: CredentialQueryRow) => void;
  onRemoveCredential: (id: number) => void;
  onToggleCredentialEnabled: (row: CredentialQueryRow) => void;
  onSetCredentialHealth: (payload: {
    credentialId: number;
    statusId?: number;
    healthKind: CredentialHealthKind;
    healthJson: Record<string, unknown> | null;
    lastError?: string | null;
  }) => void;
  onQueryUpstreamUsage: (credentialId: number) => void;
  onUpsertStatus: () => void;
  normalizeHealthKind: (value: string | undefined) => CredentialHealthKind;
  parseCooldowns: (status: CredentialStatusQueryRow | undefined) => CooldownItem[];
  healthLabel: (kind: CredentialHealthKind) => string;
  cooldownKey: (item: CooldownItem) => string;
  formatWindowLabel: (window: UsageDisplayRow["window"]) => string;
  resolveUsageGroupLabel: (label: string) => string;
  resolveLiveLimitLabel: (label: string) => string;
  t: TranslateFn;
}) {
  return (
    <div className="grid gap-3 md:grid-cols-2">
      {credentialRows.map((row) => {
        const statusList = statusesByCredential.get(row.id) ?? [];
        const primaryStatus = statusList[0];
        const healthKind = normalizeHealthKind(primaryStatus?.health_kind);
        const cooldowns = parseCooldowns(primaryStatus);
        const selectedCooldownKeys = new Set(selectedCooldownKeysByCredential[row.id] ?? []);
        const selectedCooldowns = cooldowns.filter((item) => selectedCooldownKeys.has(cooldownKey(item)));
        const showCooldowns = expandedCooldownCredentialId === row.id && healthKind === "partial";
        const usageContent = usageByCredential[row.id] ?? "";
        const liveRows = liveUsageRowsByCredential[row.id] ?? [];
        const usageDisplayKind = usageDisplayKindByCredential[row.id] ?? "calls";
        const usageDisplayRows = usageDisplayRowsByCredential[row.id] ?? [];
        const usageLoading = Boolean(usageLoadingByCredential[row.id]);
        const usageError = usageErrorByCredential[row.id];
        const showStatusEditor = statusEditorCredentialId === row.id;

        const applyCooldownDeletion = (targets: CooldownItem[]) => {
          if (targets.length === 0) {
            return;
          }
          const targetKeys = new Set(targets.map((item) => cooldownKey(item)));
          const nextModels = cooldowns.filter((item) => !targetKeys.has(cooldownKey(item)));
          onSetCredentialHealth({
            credentialId: row.id,
            statusId: primaryStatus?.id,
            healthKind: nextModels.length > 0 ? "partial" : "healthy",
            healthJson:
              nextModels.length > 0
                ? {
                    models: nextModels.map((cooldown) => ({
                      model: cooldown.model,
                      until_unix_ms: cooldown.untilUnixMs
                    }))
                  }
                : null,
            lastError: nextModels.length > 0 ? primaryStatus?.last_error : null
          });
          setSelectedCooldownKeysByCredential((prev) => {
            const next = { ...prev };
            delete next[row.id];
            return next;
          });
          if (nextModels.length === 0) {
            setExpandedCooldownCredentialId(null);
          }
        };

        return (
          <div key={row.id} className="provider-card space-y-3">
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold text-text">
                  {row.name ?? t("providers.credentialUnnamed")}
                </div>
                <div className="truncate text-xs text-muted">#{row.id}</div>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  variant={
                    healthKind === "dead"
                      ? "danger"
                      : healthKind === "partial"
                        ? "neutral"
                        : "primary"
                  }
                  onClick={() => {
                    if (healthKind === "partial") {
                      setExpandedCooldownCredentialId((prev) => (prev === row.id ? null : row.id));
                      return;
                    }
                    if (healthKind === "dead") {
                      onSetCredentialHealth({
                        credentialId: row.id,
                        statusId: primaryStatus?.id,
                        healthKind: "healthy",
                        healthJson: null,
                        lastError: null
                      });
                      return;
                    }
                    onSetCredentialHealth({
                      credentialId: row.id,
                      statusId: primaryStatus?.id,
                      healthKind: "dead",
                      healthJson: null,
                      lastError: "manually_marked_unavailable"
                    });
                  }}
                >
                  {healthLabel(healthKind)}
                </Button>
                <Button
                  variant={row.enabled ? "primary" : "neutral"}
                  onClick={() => onToggleCredentialEnabled(row)}
                >
                  {row.enabled ? t("common.enabled") : t("common.disabled")}
                </Button>
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              <Button variant="neutral" onClick={() => onEditCredential(row)}>
                {t("common.edit")}
              </Button>
              <Button variant="danger" onClick={() => onRemoveCredential(row.id)}>
                {t("common.delete")}
              </Button>
              {supportsUpstreamUsage ? (
                <Button
                  variant="neutral"
                  onClick={() => onQueryUpstreamUsage(row.id)}
                  disabled={usageLoading}
                >
                  {usageLoading ? t("common.loading") : t("providers.usage.fetch")}
                </Button>
              ) : null}
            </div>

            {showCooldowns ? (
              <div className="space-y-2 rounded-lg border border-border px-3 py-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                    {t("providers.health.cooldowns")}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="neutral"
                      disabled={selectedCooldowns.length === 0}
                      onClick={() => applyCooldownDeletion(selectedCooldowns)}
                    >
                      {t("providers.health.deleteSelected")}
                    </Button>
                    <Button
                      variant="danger"
                      disabled={cooldowns.length === 0}
                      onClick={() => applyCooldownDeletion(cooldowns)}
                    >
                      {t("providers.health.deleteAll")}
                    </Button>
                  </div>
                </div>
                {cooldowns.length === 0 ? (
                  <div className="text-xs text-muted">{t("providers.health.noCooldowns")}</div>
                ) : (
                  cooldowns.map((item) => (
                    <div
                      key={`${row.id}-${item.model}-${item.untilUnixMs}`}
                      className="flex items-center justify-between gap-2 rounded border border-border px-2 py-1"
                    >
                      <label className="flex min-w-0 items-center gap-2 text-xs text-text">
                        <input
                          type="checkbox"
                          checked={selectedCooldownKeys.has(cooldownKey(item))}
                          onChange={(event) => {
                            setSelectedCooldownKeysByCredential((prev) => {
                              const current = new Set(prev[row.id] ?? []);
                              const key = cooldownKey(item);
                              if (event.target.checked) {
                                current.add(key);
                              } else {
                                current.delete(key);
                              }
                              return {
                                ...prev,
                                [row.id]: Array.from(current)
                              };
                            });
                          }}
                        />
                        <span className="font-semibold">{item.model}</span>{" "}
                        <span className="text-muted">({new Date(item.untilUnixMs).toLocaleString()})</span>
                      </label>
                    </div>
                  ))
                )}
              </div>
            ) : null}

            {showStatusEditor ? (
              <div className="space-y-2 rounded-lg border border-border p-3">
                <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                  {t("providers.status.editor", { id: row.id })}
                </div>
                <div className="grid gap-2 md:grid-cols-2">
                  <div>
                    <Label>{t("field.idOptional")}</Label>
                    <Input value={statusForm.id} onChange={(v) => setStatusForm((p) => ({ ...p, id: v }))} />
                  </div>
                  <div>
                    <Label>{t("field.health_kind")}</Label>
                    <Input
                      value={statusForm.healthKind}
                      onChange={(v) => setStatusForm((p) => ({ ...p, healthKind: v }))}
                    />
                  </div>
                  <div>
                    <Label>{t("field.checked_at_unix_ms")}</Label>
                    <Input
                      value={statusForm.checkedAtUnixMs}
                      onChange={(v) => setStatusForm((p) => ({ ...p, checkedAtUnixMs: v }))}
                    />
                  </div>
                  <div>
                    <Label>{t("field.last_error")}</Label>
                    <Input
                      value={statusForm.lastError}
                      onChange={(v) => setStatusForm((p) => ({ ...p, lastError: v }))}
                    />
                  </div>
                  <div className="md:col-span-2">
                    <Label>{t("field.health_json")}</Label>
                    <TextArea
                      rows={4}
                      value={statusForm.healthJson}
                      onChange={(v) => setStatusForm((p) => ({ ...p, healthJson: v }))}
                    />
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button onClick={onUpsertStatus}>{t("common.save")}</Button>
                  <Button variant="neutral" onClick={() => setStatusEditorCredentialId(null)}>
                    {t("common.close")}
                  </Button>
                </div>
              </div>
            ) : null}

            {supportsUpstreamUsage &&
            (usageContent || liveRows.length > 0 || usageDisplayRows.length > 0 || usageError) ? (
              <div className="space-y-1">
                <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                  {t("providers.section.usage")}
                </div>
                {liveRows.length > 0 ? (
                  <div className="overflow-hidden rounded-lg border border-border">
                    <div className="grid grid-cols-[minmax(0,2fr)_minmax(90px,1fr)_minmax(160px,1fr)] gap-2 border-b border-border bg-card px-3 py-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                      <span>{t("providers.usage.live_limit")}</span>
                      <span>{t("providers.usage.live_percent")}</span>
                      <span>{t("providers.usage.live_reset")}</span>
                    </div>
                    <div className="divide-y divide-border">
                      {liveRows.map((item) => (
                        <div
                          key={`${row.id}-usage-live-${item.name}-${item.resetAt ?? "none"}`}
                          className="grid grid-cols-[minmax(0,2fr)_minmax(90px,1fr)_minmax(160px,1fr)] gap-2 px-3 py-2 text-xs text-text"
                        >
                          <span className="truncate">{resolveLiveLimitLabel(item.name)}</span>
                          <span>{formatUsagePercent(item.percent)}</span>
                          <span>{item.resetAt === null ? "-" : formatAtForViewer(item.resetAt)}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : usageContent ? (
                  <div className="text-xs text-muted">{t("providers.usage.live_no_limits")}</div>
                ) : null}

                {usageDisplayRows.length > 0 ? (
                  (() => {
                    const preferredWindowOrder: UsageDisplayRow["window"][] =
                      channel === "codex"
                        ? ["primary", "secondary", "code_review"]
                        : ["5h", "1d", "1w", "sum"];
                    const presentWindowSet = new Set(usageDisplayRows.map((item) => item.window));
                    const windows = preferredWindowOrder.filter((window) => presentWindowSet.has(window));
                    const byLabel = new Map<
                      string,
                      Partial<Record<UsageDisplayRow["window"], UsageDisplayRow>>
                    >();
                    for (const item of usageDisplayRows) {
                      const current = byLabel.get(item.label) ?? {};
                      current[item.window] = item;
                      byLabel.set(item.label, current);
                    }
                    const labels = Array.from(byLabel.keys()).sort((a, b) => a.localeCompare(b));

                    return (
                      <div className="space-y-2">
                        {usageDisplayKind === "tokens" ? (
                          <div className="text-xs text-muted">
                            {t("providers.usage.calls")}/{t("providers.usage.tokens_input")}/
                            {t("providers.usage.tokens_output")}/{t("providers.usage.tokens_cache")}/
                            {t("providers.usage.tokens_total")}
                          </div>
                        ) : null}
                        <div className="overflow-x-auto rounded-lg border border-border">
                          <table className="min-w-[980px] w-full border-collapse text-xs">
                            <thead>
                              <tr className="border-b border-border bg-card text-muted">
                                <th className="px-3 py-2 text-left font-semibold uppercase tracking-[0.08em]">
                                  {t("providers.usage.label")}
                                </th>
                                {windows.map((window) => (
                                  <th
                                    key={`usage-head-${window}`}
                                    className="px-3 py-2 text-left font-semibold uppercase tracking-[0.08em]"
                                  >
                                    {formatWindowLabel(window)}
                                  </th>
                                ))}
                              </tr>
                            </thead>
                            <tbody>
                              {labels.map((label) => {
                                const rowByWindow = byLabel.get(label) ?? {};
                                return (
                                  <tr
                                    key={`${row.id}-usage-row-${label}`}
                                    className="border-b border-border last:border-b-0"
                                  >
                                    <td className="px-3 py-2 font-semibold text-text">
                                      {resolveUsageGroupLabel(label)}
                                    </td>
                                    {windows.map((window) => {
                                      const item = rowByWindow[window];
                                      if (!item) {
                                        return (
                                          <td
                                            key={`${row.id}-usage-cell-${label}-${window}`}
                                            className="px-3 py-2 text-muted"
                                          >
                                            -
                                          </td>
                                        );
                                      }
                                      const rangeText = `${formatAtForViewer(item.fromUnixMs)} - ${formatAtForViewer(item.toUnixMs)}`;
                                      const cellText =
                                        usageDisplayKind === "tokens"
                                          ? `${item.calls}/${item.inputTokens}/${item.outputTokens}/${item.cacheTokens}/${item.totalTokens}`
                                          : `${item.calls}`;
                                      return (
                                        <td
                                          key={`${row.id}-usage-cell-${label}-${window}`}
                                          className="px-3 py-2 text-text whitespace-nowrap"
                                          title={rangeText}
                                        >
                                          {cellText}
                                        </td>
                                      );
                                    })}
                                  </tr>
                                );
                              })}
                            </tbody>
                          </table>
                        </div>
                      </div>
                    );
                  })()
                ) : usageContent ? (
                  <div className="text-xs text-muted">{t("providers.usage.no_calls")}</div>
                ) : null}

                {usageError ? <div className="text-xs text-amber-700">{usageError}</div> : null}

                {usageContent ? (
                  <details className="rounded-lg border border-border px-3 py-2">
                    <summary className="cursor-pointer text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                      {t("providers.usage.raw")}
                    </summary>
                    <div className="mt-2">
                      <TextArea value={usageContent} rows={8} readOnly onChange={() => {}} />
                    </div>
                  </details>
                ) : null}
              </div>
            ) : null}

            <div className="flex justify-end pt-1">
              <button
                type="button"
                className="inline-flex h-10 w-10 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text"
                onClick={() => onCopyCredential(row)}
                aria-label={t("common.copy")}
                title={t("common.copy")}
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  className="h-6 w-6"
                  aria-hidden="true"
                >
                  <rect x="9" y="9" width="11" height="11" rx="2" />
                  <path d="M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
                </svg>
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
