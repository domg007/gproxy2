import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDangerous } from "@/components/confirm-dangerous";

interface BatchToolbarProps {
  count: number;
  /** false 时隐藏启用/禁用(如统计只读)。 */
  enableDisable?: boolean;
  onEnable?: () => void;
  onDisable?: () => void;
  onDelete: () => void;
  onCancel: () => void;
  pending?: boolean;
}

/** 选择模式下的底部吸附操作条。 */
export function BatchToolbar({
  count, enableDisable = true, onEnable, onDisable, onDelete, onCancel, pending,
}: BatchToolbarProps) {
  const { t } = useTranslation();
  const [confirmOpen, setConfirmOpen] = useState(false);
  return (
    <>
      <div className="sticky bottom-0 z-10 flex items-center gap-2 rounded-md border bg-background/95 p-2 shadow-sm backdrop-blur">
        <span className="px-2 text-sm text-muted-foreground">{t("batch.selected", { count })}</span>
        <div className="ml-auto flex items-center gap-2">
          {enableDisable && (
            <>
              <Button variant="outline" size="sm" disabled={pending || count === 0} onClick={onEnable}>
                <Check className="size-4" aria-hidden />{t("batch.enable")}
              </Button>
              <Button variant="outline" size="sm" disabled={pending || count === 0} onClick={onDisable}>
                {t("batch.disable")}
              </Button>
            </>
          )}
          <Button variant="outline" size="sm" className="text-destructive" disabled={pending || count === 0} onClick={() => setConfirmOpen(true)}>
            <Trash2 className="size-4" aria-hidden />{t("batch.delete")}
          </Button>
          <Button variant="ghost" size="sm" disabled={pending} onClick={onCancel}>
            <X className="size-4" aria-hidden />{t("batch.cancel")}
          </Button>
        </div>
      </div>
      <ConfirmDangerous
        open={confirmOpen}
        onOpenChange={(o) => { if (!o) setConfirmOpen(false); }}
        title={t("batch.deleteTitle")}
        description={t("batch.deleteConfirm", { count })}
        confirmLabel={t("batch.delete")}
        onConfirm={() => { setConfirmOpen(false); onDelete(); }}
        pending={pending}
      />
    </>
  );
}
