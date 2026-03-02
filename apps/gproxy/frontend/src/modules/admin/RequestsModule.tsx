import { useEffect, useMemo, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { formatAtForViewer, parseDateTimeLocalToUnixMs } from "../../lib/datetime";
import { parseOptionalI64 } from "../../lib/form";
import { scopeAll, scopeEq } from "../../lib/scope";
import type { DownstreamRequestQueryRow, UpstreamRequestQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, SearchableSelect, Select, Table } from "../../components/ui";
import { useAdminFilterOptions } from "./hooks/useAdminFilterOptions";

function truncateText(value: string, limit: number): { preview: string; truncated: boolean } {
  if (value.length <= limit) {
    return { preview: value, truncated: false };
  }
  return { preview: value.slice(0, limit), truncated: true };
}

type PayloadPreview = {
  preview: string;
  full: string;
  truncated: boolean;
};

function EyeToggleIcon({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      aria-hidden="true"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z" />
      {open ? <circle cx="12" cy="12" r="2.5" /> : <path d="M4 20L20 4" />}
    </svg>
  );
}

function PayloadSection({ title, section }: { title: string; section: PayloadPreview }) {
  const [showFull, setShowFull] = useState(false);
  if (!section.preview) {
    return null;
  }
  const canToggle = section.truncated && section.full !== section.preview;
  const content = showFull ? section.full : section.preview;

  return (
    <div>
      <div className="mb-1 flex items-center gap-1 font-semibold text-muted">
        <span>{title}</span>
        {canToggle ? (
          <button
            type="button"
            className="inline-flex cursor-pointer items-center text-muted hover:text-text"
            aria-label={`${showFull ? "show truncated" : "show full"} ${title}`}
            onClick={() => setShowFull((value) => !value)}
          >
            <EyeToggleIcon open={showFull} />
          </button>
        ) : null}
      </div>
      <pre className="whitespace-pre-wrap break-all rounded px-2 py-1">{content}</pre>
    </div>
  );
}

function bytesToUtf8Preview(bytes: number[] | null, limit = 800): PayloadPreview {
  if (!bytes || bytes.length === 0) {
    return { preview: "", full: "", truncated: false };
  }
  try {
    const decoded = new TextDecoder().decode(new Uint8Array(bytes));
    const { preview, truncated } = truncateText(decoded, limit);
    return { preview, full: decoded, truncated };
  } catch {
    const binary = `[binary ${bytes.length} bytes]`;
    return { preview: binary, full: binary, truncated: false };
  }
}

function jsonToPreview(value: Record<string, unknown>, limit = 500): PayloadPreview {
  const text = JSON.stringify(value);
  if (!text || text === "{}") {
    return { preview: "", full: "", truncated: false };
  }
  const { preview, truncated } = truncateText(text, limit);
  return { preview, full: text, truncated };
}

export function RequestsModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [kind, setKind] = useState<"upstream" | "downstream">("upstream");
  const [rows, setRows] = useState<Array<UpstreamRequestQueryRow | DownstreamRequestQueryRow>>([]);
  const {
    isLoading: isFilterOptionsLoading,
    providerRows,
    credentialRows,
    userRows,
    userKeyRows,
    providerOptions,
    userOptions
  } = useAdminFilterOptions({
    apiKey,
    notify,
    t
  });
  const [filters, setFilters] = useState({
    providerId: "",
    credentialId: "",
    userId: "",
    userKeyId: "",
    fromAt: "",
    toAt: "",
    limit: "100"
  });

  const selectedProviderId = useMemo(() => {
    const value = Number(filters.providerId);
    return Number.isInteger(value) ? value : null;
  }, [filters.providerId]);

  const selectedUserId = useMemo(() => {
    const value = Number(filters.userId);
    return Number.isInteger(value) ? value : null;
  }, [filters.userId]);

  const providerById = useMemo(
    () => new Map(providerRows.map((row) => [row.id, row])),
    [providerRows]
  );

  const userById = useMemo(() => new Map(userRows.map((row) => [row.id, row])), [userRows]);

  const filteredCredentialOptions = useMemo(() => {
    const scopedRows =
      selectedProviderId === null
        ? credentialRows
        : credentialRows.filter((row) => row.provider_id === selectedProviderId);
    return [
      { value: "", label: t("common.all") },
      ...scopedRows.map((row) => {
        const provider = providerById.get(row.provider_id);
        const providerMeta = provider
          ? `${provider.channel} (#${provider.id})`
          : `provider #${row.provider_id}`;
        return {
          value: String(row.id),
          label: `#${row.id} · ${row.name?.trim() || t("providers.credentialUnnamed")} · ${providerMeta}`
        };
      })
    ];
  }, [credentialRows, providerById, selectedProviderId, t]);

  const filteredUserKeyOptions = useMemo(() => {
    const scopedRows =
      selectedUserId === null
        ? userKeyRows
        : userKeyRows.filter((row) => row.user_id === selectedUserId);
    return [
      { value: "", label: t("common.all") },
      ...scopedRows.map((row) => {
        const user = userById.get(row.user_id);
        const userMeta = user ? `${user.name} (#${row.user_id})` : `user #${row.user_id}`;
        const key = row.api_key.trim();
        const preview =
          key.length <= 14 ? key : `${key.slice(0, 6)}...${key.slice(-4)}`;
        return {
          value: String(row.id),
          label: `#${row.id} · ${userMeta} · ${preview}`
        };
      })
    ];
  }, [selectedUserId, t, userById, userKeyRows]);

  useEffect(() => {
    if (!filters.credentialId) {
      return;
    }
    const credentialId = Number(filters.credentialId);
    const exists = filteredCredentialOptions.some((item) => Number(item.value) === credentialId);
    if (!exists) {
      setFilters((prev) => ({ ...prev, credentialId: "" }));
    }
  }, [filteredCredentialOptions, filters.credentialId]);

  useEffect(() => {
    if (!filters.userKeyId) {
      return;
    }
    const userKeyId = Number(filters.userKeyId);
    const exists = filteredUserKeyOptions.some((item) => Number(item.value) === userKeyId);
    if (!exists) {
      setFilters((prev) => ({ ...prev, userKeyId: "" }));
    }
  }, [filteredUserKeyOptions, filters.userKeyId]);

  const query = async () => {
    try {
      const providerId = parseOptionalI64(filters.providerId);
      const credentialId = parseOptionalI64(filters.credentialId);
      const userId = parseOptionalI64(filters.userId);
      const userKeyId = parseOptionalI64(filters.userKeyId);
      const fromUnixMs = parseDateTimeLocalToUnixMs(filters.fromAt);
      const toUnixMs = parseDateTimeLocalToUnixMs(filters.toAt);
      const limit = parseOptionalI64(filters.limit) ?? 100;

      if (kind === "upstream") {
        const data = await apiRequest<UpstreamRequestQueryRow[]>("/admin/requests/upstream/query", {
          apiKey,
          method: "POST",
          body: {
            provider_id: providerId === null ? scopeAll<number>() : scopeEq(providerId),
            credential_id: credentialId === null ? scopeAll<number>() : scopeEq(credentialId),
            from_unix_ms: fromUnixMs,
            to_unix_ms: toUnixMs,
            limit
          }
        });
        setRows(data);
        return;
      }

      const data = await apiRequest<DownstreamRequestQueryRow[]>("/admin/requests/downstream/query", {
        apiKey,
        method: "POST",
        body: {
          user_id: userId === null ? scopeAll<number>() : scopeEq(userId),
          user_key_id: userKeyId === null ? scopeAll<number>() : scopeEq(userKeyId),
          from_unix_ms: fromUnixMs,
          to_unix_ms: toUnixMs,
          limit
        }
      });
      setRows(data);
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const tableColumns = [
    t("table.trace_id"),
    t("table.at"),
    t("table.status"),
    kind === "upstream" ? t("table.url") : t("table.path"),
    t("table.method"),
    t("table.payload")
  ];

  const buildPayloadCell = (row: UpstreamRequestQueryRow | DownstreamRequestQueryRow) => {
    const requestHeaders = jsonToPreview(row.request_headers_json);
    const responseHeaders = jsonToPreview(row.response_headers_json);
    const requestBody = bytesToUtf8Preview(row.request_body);
    const responseBody = bytesToUtf8Preview(row.response_body);
    const hasContent = Boolean(
      requestHeaders.preview ||
        responseHeaders.preview ||
        requestBody.preview ||
        responseBody.preview
    );

    if (!hasContent) {
      return <span className="text-xs text-muted">-</span>;
    }

    return (
      <details>
        <summary className="cursor-pointer text-xs text-muted" aria-label="toggle payload" />
        <div className="mt-2 space-y-2 text-xs">
          <PayloadSection title="req headers" section={requestHeaders} />
          <PayloadSection title="req body" section={requestBody} />
          <PayloadSection title="resp headers" section={responseHeaders} />
          <PayloadSection title="resp body" section={responseBody} />
        </div>
      </details>
    );
  };

  return (
    <Card title={t("requests.title")} subtitle={t("requests.subtitle")}>
      <div className="grid gap-3 md:grid-cols-3">
        <div>
          <Label>{t("field.kind")}</Label>
          <Select
            value={kind}
            onChange={(v) => setKind(v as "upstream" | "downstream")}
            options={[
              { value: "upstream", label: t("requests.kind.upstream") },
              { value: "downstream", label: t("requests.kind.downstream") }
            ]}
          />
        </div>
        <div>
          <Label>{t("field.provider_id")}</Label>
          <Select
            value={filters.providerId}
            onChange={(v) => setFilters((p) => ({ ...p, providerId: v }))}
            options={providerOptions}
            disabled={kind !== "upstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.credential_id")}</Label>
          <Select
            value={filters.credentialId}
            onChange={(v) => setFilters((p) => ({ ...p, credentialId: v }))}
            options={filteredCredentialOptions}
            disabled={kind !== "upstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.user_id")}</Label>
          <SearchableSelect
            value={filters.userId}
            onChange={(v) => setFilters((p) => ({ ...p, userId: v }))}
            options={userOptions}
            placeholder={t("common.all")}
            noResultLabel={t("common.none")}
            disabled={kind !== "downstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.user_key_id")}</Label>
          <SearchableSelect
            value={filters.userKeyId}
            onChange={(v) => setFilters((p) => ({ ...p, userKeyId: v }))}
            options={filteredUserKeyOptions}
            placeholder={t("common.all")}
            noResultLabel={t("common.none")}
            disabled={kind !== "downstream" || isFilterOptionsLoading}
          />
        </div>
        <div>
          <Label>{t("field.limit")}</Label>
          <Input value={filters.limit} onChange={(v) => setFilters((p) => ({ ...p, limit: v }))} />
        </div>
        <div>
          <Label>{t("field.from_at")}</Label>
          <Input
            value={filters.fromAt}
            onChange={(v) => setFilters((p) => ({ ...p, fromAt: v }))}
            placeholder={t("common.datetimePlaceholder")}
          />
        </div>
        <div>
          <Label>{t("field.to_at")}</Label>
          <Input
            value={filters.toAt}
            onChange={(v) => setFilters((p) => ({ ...p, toAt: v }))}
            placeholder={t("common.datetimePlaceholder")}
          />
        </div>
      </div>
      <div className="mt-3">
        <Button onClick={() => void query()}>{t("common.query")}</Button>
      </div>
      <div className="mt-4">
        <Table
          columns={tableColumns}
          rows={rows.map((row) => ({
            [tableColumns[0]]: row.trace_id,
            [tableColumns[1]]: formatAtForViewer(row.at),
            [tableColumns[2]]: row.response_status ?? "",
            [tableColumns[3]]:
              kind === "upstream"
                ? ((row as UpstreamRequestQueryRow).request_url ?? "")
                : (row as DownstreamRequestQueryRow).request_path,
            [tableColumns[4]]: row.request_method,
            [tableColumns[5]]: buildPayloadCell(row)
          }))}
        />
      </div>
    </Card>
  );
}
