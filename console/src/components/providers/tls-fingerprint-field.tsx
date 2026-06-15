import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon } from "lucide-react";
import { tlsPresetsQuery } from "@/api/providers";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";

const SENTINEL_DEFAULT = "__default__";
const SENTINEL_CUSTOM = "__custom__";

/** Stable stringify for deep-equality comparison of fingerprint blobs. */
function stableStringify(v: unknown): string {
  if (v === null || v === undefined) return String(v);
  if (typeof v !== "object" || Array.isArray(v)) return JSON.stringify(v);
  const obj = v as Record<string, unknown>;
  const sorted: Record<string, unknown> = {};
  for (const k of Object.keys(obj).sort()) {
    sorted[k] = obj[k];
  }
  return JSON.stringify(sorted, (_, val) =>
    val !== null && typeof val === "object" && !Array.isArray(val)
      ? Object.fromEntries(Object.entries(val as Record<string, unknown>).sort(([a], [b]) => a.localeCompare(b)))
      : val
  );
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || a === undefined || b === null || b === undefined) return false;
  return stableStringify(a) === stableStringify(b);
}

interface TlsFingerprintFieldProps {
  value: unknown;
  onChange: (v: unknown) => void;
  label?: string;
}

export function TlsFingerprintField({ value, onChange, label }: TlsFingerprintFieldProps) {
  const { t } = useTranslation("providers");
  const { data: presets = [] } = useQuery(tlsPresetsQuery);

  const isDefault = value == null;
  const matchedPreset = isDefault ? null : presets.find((p) => deepEqual(value, p.fingerprint)) ?? null;
  const isCustom = !isDefault && matchedPreset === null;

  const triggerLabel = isDefault
    ? t("tls.default").replace(" (自动)", "").replace(" (automatic)", "").replace(" (自動)", "")
    : matchedPreset
    ? matchedPreset.label
    : t("tls.customExisting");

  // Which radio is currently selected
  const radioValue = isDefault
    ? SENTINEL_DEFAULT
    : matchedPreset
    ? matchedPreset.id
    : SENTINEL_CUSTOM;

  function handleRadioChange(val: string) {
    if (val === SENTINEL_DEFAULT) {
      onChange(null);
    } else if (val === SENTINEL_CUSTOM) {
      onChange(value); // preserve the existing custom blob (no-op)
    } else {
      const preset = presets.find((p) => p.id === val);
      if (preset) onChange(preset.fingerprint);
    }
  }

  return (
    <div className="grid gap-2">
      {label && <Label>{label}</Label>}
      <Popover>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="outline"
            className="w-fit justify-between gap-2"
            aria-label={label ? `${label}: ${triggerLabel}` : triggerLabel}
          >
            <span>{triggerLabel}</span>
            <ChevronDownIcon className="size-4 text-muted-foreground" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-64 p-3">
          <p className="mb-3 text-sm font-medium">{t("tls.title")}</p>
          <RadioGroup value={radioValue} onValueChange={handleRadioChange} className="gap-2">
            <div className="flex items-center gap-2">
              <RadioGroupItem id="tls-default" value={SENTINEL_DEFAULT} />
              <Label htmlFor="tls-default" className="cursor-pointer font-normal text-sm">
                {t("tls.default")}
              </Label>
            </div>
            {presets.map((preset) => (
              <div key={preset.id} className="flex items-center gap-2">
                <RadioGroupItem id={`tls-${preset.id}`} value={preset.id} />
                <Label htmlFor={`tls-${preset.id}`} className="cursor-pointer font-normal text-sm">
                  {preset.label}
                </Label>
              </div>
            ))}
            {isCustom && (
              <div className="flex items-center gap-2">
                <RadioGroupItem id="tls-custom" value={SENTINEL_CUSTOM} />
                <Label htmlFor="tls-custom" className="cursor-pointer font-normal text-sm text-muted-foreground">
                  {t("tls.customExisting")}
                </Label>
              </div>
            )}
          </RadioGroup>
        </PopoverContent>
      </Popover>
    </div>
  );
}
