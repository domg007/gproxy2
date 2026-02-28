import { useEffect, useState } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import { parseOptionalI64 } from "../../lib/form";
import type { UserKeyQueryRow } from "../../lib/types";
import { Button, Card, Input, Label, Table } from "../../components/ui";

interface UpsertMyKeyInput {
  id?: number;
  api_key: string;
  label?: string | null;
  enabled: boolean;
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
  const [form, setForm] = useState({
    id: "",
    apiKey: "",
    label: "",
    enabled: true
  });

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

  const upsert = async () => {
    try {
      const payload: UpsertMyKeyInput = {
        api_key: form.apiKey.trim(),
        label: form.label.trim() || null,
        enabled: form.enabled
      };
      const id = parseOptionalI64(form.id);
      if (id !== null) {
        payload.id = id;
      }
      await apiRequest("/user/keys/upsert", {
        apiKey,
        method: "POST",
        body: payload
      });
      notify("success", t("myKeys.saved"));
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

  const tableColumns = [t("table.id"), t("table.user_id"), t("table.api_key"), t("common.action")];

  return (
    <div className="space-y-4">
      <Card
        title={t("myKeys.title")}
        subtitle={t("myKeys.subtitle")}
        action={
          <Button variant="secondary" onClick={() => void load()}>
            {t("common.refresh")}
          </Button>
        }
      >
        <Table
          columns={tableColumns}
          rows={rows.map((row) => ({
            [tableColumns[0]]: row.id,
            [tableColumns[1]]: row.user_id,
            [tableColumns[2]]: row.api_key,
            [tableColumns[3]]: (
              <Button variant="danger" onClick={() => void remove(row.id)}>
                {t("common.delete")}
              </Button>
            )
          }))}
        />
      </Card>
      <Card title={t("myKeys.upsert")}>
        <div className="grid gap-3 md:grid-cols-2">
          <div>
            <Label>{t("field.idOptional")}</Label>
            <Input value={form.id} onChange={(v) => setForm((p) => ({ ...p, id: v }))} />
          </div>
          <div>
            <Label>{t("field.api_key")}</Label>
            <Input value={form.apiKey} onChange={(v) => setForm((p) => ({ ...p, apiKey: v }))} />
            <p className="mt-1 text-xs text-muted">{t("myKeys.prefixHint")}</p>
          </div>
          <div>
            <Label>{t("field.labelOptional")}</Label>
            <Input value={form.label} onChange={(v) => setForm((p) => ({ ...p, label: v }))} />
          </div>
          <div className="flex items-end gap-2 pb-2">
            <input
              id="my-key-enabled"
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => setForm((p) => ({ ...p, enabled: e.target.checked }))}
            />
            <label htmlFor="my-key-enabled" className="text-sm text-muted">
              {t("common.enabled")}
            </label>
          </div>
        </div>
        <div className="mt-3">
          <Button onClick={() => void upsert()}>{t("common.save")}</Button>
        </div>
      </Card>
    </div>
  );
}
