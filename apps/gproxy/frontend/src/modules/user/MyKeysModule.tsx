import { useEffect, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { copyTextToClipboard } from "../../lib/clipboard";
import type { UserKeyQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, Table } from "../../components/ui";

interface GeneratedMyKey {
  id: number;
  user_id: number;
  api_key: string;
}

export function MyKeysModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<UserKeyQueryRow[]>([]);
  const [revealedKeyIds, setRevealedKeyIds] = useState<Set<number>>(() => new Set());
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmNewPassword, setConfirmNewPassword] = useState("");

  const maskApiKey = (value: string): string => {
    const key = value.trim();
    if (!key) {
      return "";
    }
    if (key.length <= 8) {
      return "****";
    }
    return `${key.slice(0, 4)}${"*".repeat(Math.max(4, key.length - 8))}${key.slice(-4)}`;
  };

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

  const copyKey = async (key: string) => {
    try {
      await copyTextToClipboard(key);
      notify("success", t("common.copied"));
    } catch {
      notify("error", t("common.copyFailed"));
    }
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

  const load = async () => {
    try {
      const data = await apiRequest<UserKeyQueryRow[]>("/user/keys/query", {
        apiKey,
        method: "POST"
      });
      setRows([...data].sort((a, b) => a.id - b.id));
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const generate = async () => {
    try {
      const generated = await apiRequest<GeneratedMyKey>("/user/keys/generate", {
        apiKey,
        method: "POST"
      });
      notify("success", t("myKeys.generated", { key: generated.api_key }));
      await load();
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const remove = async (id: number) => {
    try {
      await apiRequest("/user/keys/delete", {
        apiKey,
        method: "POST",
        body: { id }
      });
      notify("success", t("myKeys.deleted"));
      await load();
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const changeMyPassword = async () => {
    if (!currentPassword.trim()) {
      notify("error", t("myPassword.currentRequired"));
      return;
    }
    if (!newPassword.trim()) {
      notify("error", t("myPassword.newRequired"));
      return;
    }
    if (newPassword !== confirmNewPassword) {
      notify("error", t("myPassword.confirmMismatch"));
      return;
    }

    try {
      await apiRequest("/user/password/change", {
        apiKey,
        method: "POST",
        body: {
          current_password: currentPassword,
          new_password: newPassword
        }
      });
      setCurrentPassword("");
      setNewPassword("");
      setConfirmNewPassword("");
      notify("success", t("myPassword.changed"));
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const tableColumns = [t("table.id"), t("table.user_id"), t("table.api_key"), t("common.action")];

  return (
    <div className="space-y-4">
      <Card title={t("myPassword.title")} subtitle={t("myPassword.subtitle")}>
        <div className="grid gap-3 md:grid-cols-3">
          <div>
            <Label>{t("myPassword.current")}</Label>
            <Input
              type="password"
              value={currentPassword}
              onChange={setCurrentPassword}
              placeholder={t("myPassword.currentPlaceholder")}
            />
          </div>
          <div>
            <Label>{t("myPassword.new")}</Label>
            <Input
              type="password"
              value={newPassword}
              onChange={setNewPassword}
              placeholder={t("myPassword.newPlaceholder")}
            />
          </div>
          <div>
            <Label>{t("myPassword.confirm")}</Label>
            <Input
              type="password"
              value={confirmNewPassword}
              onChange={setConfirmNewPassword}
              placeholder={t("myPassword.confirmPlaceholder")}
            />
          </div>
        </div>
        <div className="mt-3">
          <Button onClick={() => void changeMyPassword()}>{t("myPassword.submit")}</Button>
        </div>
      </Card>

      <Card
        title={t("myKeys.title")}
        subtitle={t("myKeys.subtitle")}
        action={
          <div className="flex items-center gap-2">
            <Button variant="primary" onClick={() => void generate()}>
              {t("myKeys.generate")}
            </Button>
            <Button variant="secondary" onClick={() => void load()}>
              {t("common.refresh")}
            </Button>
          </div>
        }
      >
        <Table
          columns={tableColumns}
          rows={rows.map((row) => ({
            [tableColumns[0]]: row.id,
            [tableColumns[1]]: row.user_id,
            [tableColumns[2]]: (
              <div className="flex items-center gap-2">
                <span className="font-mono text-xs">
                  {revealedKeyIds.has(row.id) ? row.api_key : maskApiKey(row.api_key)}
                </span>
                {renderVisibilityButton(row.id)}
                {renderCopyButton(row.api_key)}
              </div>
            ),
            [tableColumns[3]]: (
              <Button variant="danger" onClick={() => void remove(row.id)}>
                {t("common.delete")}
              </Button>
            )
          }))}
        />
      </Card>
    </div>
  );
}
