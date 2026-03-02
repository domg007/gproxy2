import { useEffect, useMemo, useState } from "react";

import { useI18n } from "../../../app/i18n";
import { copyTextToClipboard } from "../../../lib/clipboard";
import type { UserKeyQueryRow, UserQueryRow } from "../../../lib/types";
import { Button, Select } from "../../../components/ui";
import { maskApiKey } from "./helpers";

type UserKeysPaneProps = {
  selectedUser: UserQueryRow | null;
  selectedUserId: number | null;
  keyRows: UserKeyQueryRow[];
  notify: (kind: "success" | "error" | "info", message: string) => void;
  onGenerateKey: () => void;
  onRefreshKeys: () => void;
  onDeleteKey: (id: number) => void;
};

function defaultPageSizeByViewport(): number {
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

export function UserKeysPane({
  selectedUser,
  selectedUserId,
  keyRows,
  notify,
  onGenerateKey,
  onRefreshKeys,
  onDeleteKey
}: UserKeysPaneProps) {
  const { t } = useI18n();
  const [revealedKeyIds, setRevealedKeyIds] = useState<Set<number>>(() => new Set());
  const [pageSize, setPageSize] = useState<number>(() => defaultPageSizeByViewport());
  const [page, setPage] = useState(1);

  useEffect(() => {
    setPage(1);
    setRevealedKeyIds(new Set());
  }, [selectedUserId]);

  useEffect(() => {
    setPage(1);
  }, [pageSize]);

  const totalPages = Math.max(1, Math.ceil(keyRows.length / pageSize));

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  const pagedKeyRows = useMemo(() => {
    const start = (page - 1) * pageSize;
    return keyRows.slice(start, start + pageSize);
  }, [keyRows, page, pageSize]);

  const toggleKeyVisibility = (id: number) => {
    setRevealedKeyIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const renderVisibilityButton = (id: number) => {
    const shown = revealedKeyIds.has(id);
    return (
      <button
        type="button"
        className="relative z-10 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text"
        onClick={() => toggleKeyVisibility(id)}
        aria-label={shown ? t("common.hide") : t("common.show")}
        title={shown ? t("common.hide") : t("common.show")}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          className="h-4 w-4"
          aria-hidden="true"
        >
          <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6Z" />
          <circle cx="12" cy="12" r="2.8" />
          {shown ? null : <path d="M4 20L20 4" />}
        </svg>
      </button>
    );
  };

  const copyKey = async (key: string) => {
    try {
      await copyTextToClipboard(key);
      notify("success", t("common.copied"));
    } catch {
      notify("error", t("common.copyFailed"));
    }
  };

  const renderCopyButton = (key: string) => {
    return (
      <button
        type="button"
        className="relative z-10 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text"
        onClick={() => void copyKey(key)}
        aria-label={t("common.copy")}
        title={t("common.copy")}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          className="h-4 w-4"
          aria-hidden="true"
        >
          <rect x="9" y="9" width="11" height="11" rx="2" />
          <path d="M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
        </svg>
      </button>
    );
  };

  return (
    <div className="provider-card">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-sm font-semibold text-text">{t("users.selectedKeys")}</div>
          <div className="text-xs text-muted">
            {selectedUser
              ? t("users.selectedUserMeta", { id: selectedUser.id, name: selectedUser.name })
              : t("users.selectHint")}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" disabled={selectedUserId === null} onClick={onGenerateKey}>
            {t("users.generateKey")}
          </Button>
          <Button variant="neutral" disabled={selectedUserId === null} onClick={onRefreshKeys}>
            {t("users.refreshKeys")}
          </Button>
        </div>
      </div>

      <div className="mt-3 space-y-3">
        {keyRows.length === 0 ? (
          <div className="text-sm text-muted">{t("users.noKeys")}</div>
        ) : (
          <>
            {pagedKeyRows.map((row) => (
              <div key={row.id} className="provider-card">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="text-sm font-semibold text-text">{t("users.keyTitle", { id: row.id })}</div>
                    <div className="mt-1 flex min-w-0 items-center gap-2">
                      <div className="min-w-0 flex-1 truncate font-mono text-xs text-muted">
                        {revealedKeyIds.has(row.id) ? row.api_key : maskApiKey(row.api_key)}
                      </div>
                      {renderVisibilityButton(row.id)}
                      {renderCopyButton(row.api_key)}
                    </div>
                  </div>
                  <div className="flex shrink-0 flex-wrap gap-2">
                    <Button variant="danger" onClick={() => onDeleteKey(row.id)}>
                      {t("common.delete")}
                    </Button>
                  </div>
                </div>
              </div>
            ))}
            <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
              <div>
                {t("common.pager.stats", {
                  shown: pagedKeyRows.length,
                  total: keyRows.length
                })}
              </div>
              <div className="flex items-center gap-2">
                <span>{t("common.show")}</span>
                <div className="w-20">
                  <Select
                    value={String(pageSize)}
                    onChange={(value) => setPageSize(Number(value))}
                    options={[
                      { value: "5", label: "5" },
                      { value: "10", label: "10" },
                      { value: "20", label: "20" },
                      { value: "50", label: "50" }
                    ]}
                  />
                </div>
                <Button
                  variant="neutral"
                  disabled={page <= 1}
                  onClick={() => setPage((prev) => Math.max(1, prev - 1))}
                >
                  {t("users.pager.prev")}
                </Button>
                <span>{t("common.pager.page", { current: page, total: totalPages })}</span>
                <Button
                  variant="neutral"
                  disabled={page >= totalPages}
                  onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))}
                >
                  {t("users.pager.next")}
                </Button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
