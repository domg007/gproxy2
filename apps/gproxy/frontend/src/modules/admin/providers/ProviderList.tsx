import type { ProviderQueryRow } from "../../../lib/types";
import { Button } from "../../../components/ui";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

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
  onDelete: (id: number) => void;
  t: TranslateFn;
}) {
  return (
    <aside className="space-y-3">
      <div className="text-sm font-semibold text-text">{t("providers.list")}</div>
      {providerRows.length === 0 ? (
        <div className="provider-card text-sm text-muted">{t("providers.empty")}</div>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
          {providerRows.map((row) => {
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
                      onDelete(row.id);
                    }}
                  >
                    {t("providers.delete")}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </aside>
  );
}

