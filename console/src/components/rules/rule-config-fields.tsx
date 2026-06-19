import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronsUpDown } from "lucide-react";
import { JsonField, parseJsonText } from "@/components/form/json-field";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { SystemTextFields } from "./config/system-text";
import { RewriteFields } from "./config/rewrite";
import { SanitizeFields } from "./config/sanitize";
import { CacheBreakpointFields } from "./config/cache-breakpoint";
import { HeaderFields } from "./config/header";

interface Props {
  kind: string;
  value: unknown;
  onChange: (v: unknown) => void;
  onValidChange?: (valid: boolean) => void;
}

export function RuleConfigFields({ kind, value, onChange, onValidChange }: Props) {
  const { t } = useTranslation("rules");
  const [rawValid, setRawValid] = useState(true);

  const handleRawChange = (text: string) => {
    const parsed = parseJsonText(text);
    const ok = parsed.ok;
    setRawValid(ok);
    onValidChange?.(ok);
    if (ok) onChange(parsed.value);
  };

  const v = (value ?? {}) as Record<string, unknown>;

  return (
    <div className="grid gap-3">
      {kind === "system_text" && <SystemTextFields value={v} onChange={onChange} />}
      {kind === "rewrite" && <RewriteFields value={v} onChange={onChange} onValidChange={onValidChange} />}
      {kind === "sanitize" && <SanitizeFields value={v} onChange={onChange} />}
      {kind === "cache_breakpoint" && <CacheBreakpointFields value={v} onChange={onChange} />}
      {kind === "header" && <HeaderFields value={v} onChange={onChange} />}

      <Collapsible>
        <CollapsibleTrigger className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
          <ChevronsUpDown className="size-3" aria-hidden /> {t("advanced")}
        </CollapsibleTrigger>
        <CollapsibleContent className="pt-2">
          <JsonField value={JSON.stringify(value ?? {}, null, 2)} onChange={handleRawChange} rows={8} />
          {!rawValid && <p className="text-xs text-destructive">{t("rule.rawJsonError")}</p>}
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
