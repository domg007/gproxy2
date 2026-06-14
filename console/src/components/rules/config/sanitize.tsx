import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface SanitizeValue { pattern?: string; replacement?: string }

interface Props {
  value: SanitizeValue;
  onChange: (v: unknown) => void;
}

export function SanitizeFields({ value, onChange }: Props) {
  const { t } = useTranslation("rules");
  return (
    <div className="grid gap-3">
      <div className="grid gap-1">
        <Label htmlFor="cfg-san-pattern">{t("config.pattern")}</Label>
        <Input
          id="cfg-san-pattern"
          value={value.pattern ?? ""}
          onChange={(e) => onChange({ ...value, pattern: e.target.value })}
          placeholder="sensitive_\w+"
        />
      </div>
      <div className="grid gap-1">
        <Label htmlFor="cfg-san-repl">{t("config.replacement")}</Label>
        <Input
          id="cfg-san-repl"
          value={value.replacement ?? ""}
          onChange={(e) => onChange({ ...value, replacement: e.target.value })}
          placeholder="[REDACTED]"
        />
      </div>
    </div>
  );
}
