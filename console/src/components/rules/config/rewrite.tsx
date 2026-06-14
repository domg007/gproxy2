import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { JsonField, parseJsonText } from "@/components/form/json-field";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

interface RewriteValue { path?: string; action?: string; value_json?: unknown }

interface Props {
  value: RewriteValue;
  onChange: (v: unknown) => void;
  onValidChange?: (valid: boolean) => void;
}

export function RewriteFields({ value, onChange, onValidChange }: Props) {
  const { t } = useTranslation("rules");
  const action = value.action ?? "set";
  const showValue = action === "set" || action === "merge";

  const [valueText, setValueText] = useState(
    value.value_json !== undefined ? JSON.stringify(value.value_json, null, 2) : "",
  );

  // Reset valueText when action changes to delete
  useEffect(() => {
    if (!showValue) {
      onValidChange?.(true);
      // Drop value_json from the parent config when switching to delete
      const { value_json: _v, ...rest } = value;
      onChange({ ...rest });
    }
    // Only trigger on showValue change (driven by action)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showValue]);

  const handleValueText = (text: string) => {
    setValueText(text);
    if (text.trim() === "") {
      // optional — treat empty as no value_json
      const { value_json: _v, ...rest } = value;
      onChange({ ...rest });
      onValidChange?.(true);
    } else {
      const parsed = parseJsonText(text);
      if (parsed.ok) {
        onChange({ ...value, value_json: parsed.value });
        onValidChange?.(true);
      } else {
        onValidChange?.(false);
      }
    }
  };

  return (
    <div className="grid gap-3">
      <div className="grid gap-1">
        <Label htmlFor="cfg-rw-path">{t("config.path")}</Label>
        <Input
          id="cfg-rw-path"
          value={value.path ?? ""}
          onChange={(e) => onChange({ ...value, path: e.target.value })}
          placeholder="$.messages[0].content"
        />
      </div>
      <div className="grid gap-1">
        <Label htmlFor="cfg-rw-action">{t("config.action")}</Label>
        <Select
          value={action}
          onValueChange={(v) => onChange({ ...value, action: v })}
        >
          <SelectTrigger id="cfg-rw-action"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="set">{t("config.actionOptions.set")}</SelectItem>
            <SelectItem value="delete">{t("config.actionOptions.delete")}</SelectItem>
            <SelectItem value="merge">{t("config.actionOptions.merge")}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      {showValue && (
        <div className="grid gap-1">
          <Label htmlFor="cfg-rw-val">{t("config.value")}</Label>
          <JsonField
            id="cfg-rw-val"
            value={valueText}
            onChange={handleValueText}
            rows={4}
          />
        </div>
      )}
    </div>
  );
}
