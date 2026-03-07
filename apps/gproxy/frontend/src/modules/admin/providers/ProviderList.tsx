import { useEffect, useMemo, useState } from "react";

import type { ProviderQueryRow } from "../../../lib/types";
import { Button, Input, Select } from "../../../components/ui";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

type ProviderSearchMode = "id" | "name";

function defaultProviderPageSize(): number {
  if (typeof window === "undefined") {
    return 20;
  }
  if (window.innerWidth < 640) {
    return 5;
  }
  if (window.innerWidth < 1024) {
    return 10;
  }
  if (window.innerWidth < 1600) {
    return 20;
  }
  return 50;
}

export function ProviderList({
  providerRows,
  selectedProviderId,
  onSelectProvider,
  onToggleEnabled,
  onEdit,
  onDelete,
  t
}: {
  providerRows: ProviderQueryRow[];
  selectedProviderId: number | null;
  onSelectProvider: (row: ProviderQueryRow) => void;
  onToggleEnabled: (row: ProviderQueryRow) => void;
  onEdit: (row: ProviderQueryRow) => void;
  onDelete: (row: ProviderQueryRow) => void;
  t: TranslateFn;
}) {
  const [searchMode, setSearchMode] = useState<ProviderSearchMode>("name");
  const [searchText, setSearchText] = useState("");
  const [pageSize, setPageSize] = useState<number>(() => defaultProviderPageSize());
  const [page, setPage] = useState(1);

  const filteredRows = useMemo(() => {
    const needle = searchText.trim().toLowerCase();
    if (!needle) {
      return providerRows;
    }
    return providerRows.filter((row) => {
      if (searchMode === "id") {
        return String(row.id).includes(needle);
      }
      return row.name.toLowerCase().includes(needle);
    });
  }, [providerRows, searchMode, searchText]);

  useEffect(() => {
    setPage(1);
  }, [searchMode, searchText, pageSize]);

  const totalPages = Math.max(1, Math.ceil(filteredRows.length / pageSize));

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const pagedRows = useMemo(() => {
    const start = (page - 1) * pageSize;
    return filteredRows.slice(start, start + pageSize);
  }, [filteredRows, page, pageSize]);

  const pageSizeOptions = [5, 10, 20, 50].map((size) => ({
    value: String(size),
    label: `${size}`
  }));

  return (
    <aside className="space-y-3">
      <div className="text-sm font-semibold text-text">{t("providers.list")}</div>
      <div className="flex flex-wrap items-end gap-2 sm:flex-nowrap">
        <div className="w-20">
          <Select
            value={searchMode}
            onChange={(value) => setSearchMode(value as ProviderSearchMode)}
            options={[
              { value: "id", label: t("providers.search.mode.id") },
              { value: "name", label: t("providers.search.mode.name") }
            ]}
          />
        </div>
        <div className="min-w-[120px] flex-1 sm:min-w-[140px]">
          <Input
            value={searchText}
            onChange={setSearchText}
            placeholder={t("providers.search.placeholder.provider")}
          />
        </div>
        <div className="w-16">
          <Select
            value={String(pageSize)}
            onChange={(value) => setPageSize(Number(value))}
            options={pageSizeOptions}
          />
        </div>
      </div>
      {providerRows.length === 0 ? (
        <div className="provider-card text-sm text-muted">{t("providers.empty")}</div>
      ) : filteredRows.length === 0 ? (
        <div className="provider-card text-sm text-muted">{t("providers.search.emptyProvider")}</div>
      ) : (
        <>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
            {pagedRows.map((row) => {
              const selected = selectedProviderId === row.id;
              return (
                <div
                  key={row.id}
                  className={`provider-card cursor-pointer ${selected ? "provider-card-active" : ""}`}
                  role="button"
                  tabIndex={0}
                  onClick={() => onSelectProvider(row)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onSelectProvider(row);
                    }
                  }}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="min-w-0 flex-1 text-left">
                      <div className="truncate text-sm font-semibold text-text">{row.name}</div>
                      <div className="truncate text-xs text-muted">
                        #{row.id} · {row.channel}
                      </div>
                    </div>
                    <button
                      type="button"
                      className={`badge ${row.enabled ? "badge-active" : ""} cursor-pointer`}
                      onClick={(event) => {
                        event.stopPropagation();
                        onToggleEnabled(row);
                      }}
                    >
                      {row.enabled ? t("common.enabled") : t("common.disabled")}
                    </button>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button
                      variant="neutral"
                      onClick={(event) => {
                        event.stopPropagation();
                        onEdit(row);
                      }}
                    >
                      {t("providers.edit")}
                    </Button>
                    <Button
                      variant="danger"
                      onClick={(event) => {
                        event.stopPropagation();
                        onDelete(row);
                      }}
                    >
                      {t("providers.delete")}
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
          <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
            <div>
              {t("providers.pager.stats", {
                shown: pagedRows.length,
                total: filteredRows.length
              })}
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="neutral"
                disabled={page <= 1}
                onClick={() => setPage((prev) => Math.max(1, prev - 1))}
              >
                {t("providers.pager.prev")}
              </Button>
              <span>{t("providers.pager.page", { current: page, total: totalPages })}</span>
              <Button
                variant="neutral"
                disabled={page >= totalPages}
                onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))}
              >
                {t("providers.pager.next")}
              </Button>
            </div>
          </div>
        </>
      )}
    </aside>
  );
}
