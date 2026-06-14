import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

interface CacheBreakpointValue { target?: string; index?: number; ttl?: string }

interface Props {
  value: CacheBreakpointValue;
  onChange: (v: unknown) => void;
}

export function CacheBreakpointFields({ value, onChange }: Props) {
  const { t } = useTranslation("rules");
  return (
    <div className="grid gap-3">
      <div className="grid gap-1">
        <Label htmlFor="cfg-cb-target">{t("config.target")}</Label>
        <Select
          value={value.target ?? "system"}
          onValueChange={(v) => onChange({ ...value, target: v })}
        >
          <SelectTrigger id="cfg-cb-target"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="system">{t("config.targetOptions.system")}</SelectItem>
            <SelectItem value="tools">{t("config.targetOptions.tools")}</SelectItem>
            <SelectItem value="last_message">{t("config.targetOptions.last_message")}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="grid gap-1">
        <Label htmlFor="cfg-cb-index">{t("config.index")}</Label>
        <Input
          id="cfg-cb-index"
          type="number"
          value={value.index ?? ""}
          onChange={(e) => {
            const n = e.target.value.trim();
            onChange({ ...value, index: n === "" ? undefined : Number(n) });
          }}
          placeholder="optional"
        />
      </div>
      <div className="grid gap-1">
        <Label htmlFor="cfg-cb-ttl">{t("config.ttl")}</Label>
        <Select
          value={value.ttl ?? ""}
          onValueChange={(v) => onChange({ ...value, ttl: v || undefined })}
        >
          <SelectTrigger id="cfg-cb-ttl"><SelectValue placeholder="no TTL" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="">no TTL</SelectItem>
            <SelectItem value="5m">5m</SelectItem>
            <SelectItem value="1h">1h</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
