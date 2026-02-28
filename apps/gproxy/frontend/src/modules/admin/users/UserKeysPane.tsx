import { useI18n } from "../../../app/i18n";
import type { UserKeyQueryRow, UserQueryRow } from "../../../lib/types";
import { Button, Input, Label } from "../../../components/ui";
import { maskApiKey } from "./helpers";
import type { UserKeyFormState } from "./types";

type UserKeysPaneProps = {
  selectedUser: UserQueryRow | null;
  selectedUserId: number | null;
  keyRows: UserKeyQueryRow[];
  showKeyEditor: boolean;
  keyForm: UserKeyFormState;
  onToggleEditor: () => void;
  onRefreshKeys: () => void;
  onChangeKeyForm: (patch: Partial<UserKeyFormState>) => void;
  onSaveKey: () => void;
  onEditKey: (row: UserKeyQueryRow) => void;
  onDeleteKey: (id: number) => void;
};

export function UserKeysPane({
  selectedUser,
  selectedUserId,
  keyRows,
  showKeyEditor,
  keyForm,
  onToggleEditor,
  onRefreshKeys,
  onChangeKeyForm,
  onSaveKey,
  onEditKey,
  onDeleteKey
}: UserKeysPaneProps) {
  const { t } = useI18n();

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
          <Button variant={showKeyEditor ? "neutral" : "primary"} disabled={selectedUserId === null} onClick={onToggleEditor}>
            {showKeyEditor ? t("common.cancel") : t("users.addKey")}
          </Button>
          <Button variant="neutral" disabled={selectedUserId === null} onClick={onRefreshKeys}>
            {t("users.refreshKeys")}
          </Button>
        </div>
      </div>

      {showKeyEditor ? (
        <div className="mt-3 space-y-3 rounded-xl border border-border p-3">
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <Label>{t("field.id")}</Label>
              <Input value={keyForm.id} onChange={(value) => onChangeKeyForm({ id: value })} />
            </div>
            <div>
              <Label>{t("field.user_id")}</Label>
              <Input value={selectedUserId === null ? "" : String(selectedUserId)} onChange={() => {}} readOnly />
            </div>
            <div className="md:col-span-2">
              <Label>{t("field.api_key")}</Label>
              <Input value={keyForm.apiKey} onChange={(value) => onChangeKeyForm({ apiKey: value })} />
              {selectedUserId !== null ? (
                <p className="mt-1 text-xs text-muted">
                  {t("users.keyPrefixHint", { prefix: `u${selectedUserId}_` })}
                </p>
              ) : null}
            </div>
            <div>
              <Label>{t("field.labelOptional")}</Label>
              <Input value={keyForm.label} onChange={(value) => onChangeKeyForm({ label: value })} />
            </div>
            <div className="flex items-end gap-2 pb-1">
              <input
                id="user-key-enabled"
                type="checkbox"
                checked={keyForm.enabled}
                onChange={(event) => onChangeKeyForm({ enabled: event.target.checked })}
              />
              <label htmlFor="user-key-enabled" className="text-sm text-muted">
                {t("common.enabled")}
              </label>
            </div>
          </div>

          <Button onClick={onSaveKey} disabled={selectedUserId === null}>
            {t("common.save")}
          </Button>
        </div>
      ) : null}

      <div className="mt-3 space-y-3">
        {keyRows.length === 0 ? (
          <div className="text-sm text-muted">{t("users.noKeys")}</div>
        ) : (
          keyRows.map((row) => (
            <div key={row.id} className="provider-card">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-sm font-semibold text-text">{t("users.keyTitle", { id: row.id })}</div>
                  <div className="truncate font-mono text-xs text-muted">{maskApiKey(row.api_key)}</div>
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  <Button variant="neutral" onClick={() => onEditKey(row)}>
                    {t("common.edit")}
                  </Button>
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
