import { useState } from "react";

import { useI18n } from "../../../app/i18n";
import { copyTextToClipboard } from "../../../lib/clipboard";
import type { UserKeyQueryRow, UserQueryRow } from "../../../lib/types";
import { Button } from "../../../components/ui";
import { maskApiKey } from "./helpers";

type UserKeysPaneProps = {
  selectedUser: UserQueryRow | null;
  selectedUserId: number | null;
  keyRows: UserKeyQueryRow[];
  onGenerateKey: () => void;
  onRefreshKeys: () => void;
  onDeleteKey: (id: number) => void;
};

export function UserKeysPane({
  selectedUser,
  selectedUserId,
  keyRows,
  onGenerateKey,
  onRefreshKeys,
  onDeleteKey
}: UserKeysPaneProps) {
  const { t } = useI18n();
  const [revealedKeyIds, setRevealedKeyIds] = useState<Set<number>>(() => new Set());

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
        className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text"
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

  const renderCopyButton = (key: string) => {
    return (
      <button
        type="button"
        className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-border bg-panel-muted text-muted transition hover:text-text"
        onClick={() => {
          void copyTextToClipboard(key).catch(() => {});
        }}
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
          keyRows.map((row) => (
            <div key={row.id} className="provider-card">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-sm font-semibold text-text">{t("users.keyTitle", { id: row.id })}</div>
                  <div className="mt-1 flex items-center gap-2">
                    <div className="truncate font-mono text-xs text-muted">
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
          ))
        )}
      </div>
    </div>
  );
}
