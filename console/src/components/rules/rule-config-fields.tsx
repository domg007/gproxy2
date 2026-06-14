import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { JsonField, parseJsonText } from "@/components/form/json-field";
import { Button } from "@/components/ui/button";
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
  const [rawMode, setRawMode] = useState(false);
  const [rawText, setRawText] = useState("");
  const [rawValid, setRawValid] = useState(true);

  // When kind changes while in raw mode, exit raw mode
  useEffect(() => {
    if (rawMode) {
      setRawMode(false);
      onValidChange?.(true);
    }
    // Only trigger on kind change, not rawMode change
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);

  const enterRaw = () => {
    setRawText(JSON.stringify(value ?? {}, null, 2));
    setRawValid(true);
    setRawMode(true);
  };

  const exitRaw = () => {
    const parsed = parseJsonText(rawText);
    if (parsed.ok) {
      onChange(parsed.value);
      setRawMode(false);
      onValidChange?.(true);
    } else {
      setRawValid(false);
    }
  };

  const handleRawChange = (text: string) => {
    setRawText(text);
    const parsed = parseJsonText(text);
    const ok = parsed.ok;
    setRawValid(ok);
    onValidChange?.(ok);
    if (ok) onChange(parsed.value);
  };

  const v = (value ?? {}) as Record<string, unknown>;

  return (
    <div className="grid gap-3">
      <div className="flex justify-end">
        {rawMode ? (
          <Button type="button" variant="ghost" size="sm" onClick={exitRaw}>
            {t("rule.rawJson")} (structured)
          </Button>
        ) : (
          <Button type="button" variant="ghost" size="sm" onClick={enterRaw}>
            {t("rule.rawJson")}
          </Button>
        )}
      </div>

      {rawMode ? (
        <div className="grid gap-1">
          <JsonField
            value={rawText}
            onChange={handleRawChange}
            rows={8}
          />
          {!rawValid && (
            <p className="text-xs text-destructive">
              Fix JSON errors to switch back to structured view.
            </p>
          )}
        </div>
      ) : (
        <>
          {kind === "system_text" && (
            <SystemTextFields value={v} onChange={onChange} />
          )}
          {kind === "rewrite" && (
            <RewriteFields value={v} onChange={onChange} onValidChange={onValidChange} />
          )}
          {kind === "sanitize" && (
            <SanitizeFields value={v} onChange={onChange} />
          )}
          {kind === "cache_breakpoint" && (
            <CacheBreakpointFields value={v} onChange={onChange} />
          )}
          {kind === "header" && (
            <HeaderFields value={v} onChange={onChange} />
          )}
        </>
      )}
    </div>
  );
}
