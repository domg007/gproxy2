import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { upsertCredential, type CredentialView } from "@/api/credentials";
import { ApiError } from "@/api/http";
import { buildSecret, SecretEditor, secretTemplateText } from "@/components/providers/secret-editor";
import { JsonField, parseJsonText } from "@/components/form/json-field";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

interface CredentialFormProps {
  providerId: number;
  channel: string;
  /** undefined = create */
  credential?: CredentialView;
  onSaved: () => void;
}

function intOrNull(v: string): number | null {
  const n = Number(v);
  return v.trim() !== "" && Number.isInteger(n) && n > 0 ? n : null;
}

export function CredentialForm({ providerId, channel, credential, onSaved }: CredentialFormProps) {
  const { t } = useTranslation("providers");
  const { t: tc } = useTranslation("common"); // json.invalid lives in common
  const queryClient = useQueryClient();
  const editing = credential !== undefined;

  const [label, setLabel] = useState(credential?.label ?? "");
  const [secretText, setSecretText] = useState(editing ? "" : secretTemplateText(channel));
  const [weight, setWeight] = useState(String(credential?.weight ?? 100));
  const [rpm, setRpm] = useState(credential?.rpm_limit?.toString() ?? "");
  const [tpm, setTpm] = useState(credential?.tpm_limit?.toString() ?? "");
  const [proxyUrl, setProxyUrl] = useState(credential?.proxy_url ?? "");
  const [tlsText, setTlsText] = useState(
    credential?.tls_fingerprint == null ? "" : JSON.stringify(credential.tls_fingerprint, null, 2),
  );
  const [enabled, setEnabled] = useState(credential?.enabled ?? true);
  const [formError, setFormError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () => {
      const secret = buildSecret(channel, secretText);
      if (!editing && secret === null) {
        throw new ApiError(0, "bad_request", tc("json.invalid"));
      }
      if (editing && secretText.trim() !== "" && secret === null) {
        throw new ApiError(0, "bad_request", tc("json.invalid"));
      }
      const tls = tlsText.trim() === "" ? { ok: true as const, value: null } : parseJsonText(tlsText);
      if (!tls.ok) throw new ApiError(0, "bad_request", tc("json.invalid"));
      return upsertCredential(providerId, {
        id: credential?.id ?? null,
        label: label.trim() === "" ? null : label.trim(),
        kind: credential?.kind ?? "api_key",
        ...(secret !== null ? { secret_json: secret } : {}),
        weight: intOrNull(weight) ?? 100,
        rpm_limit: intOrNull(rpm),
        tpm_limit: intOrNull(tpm),
        proxy_url: proxyUrl.trim() === "" ? null : proxyUrl.trim(),
        // Omit when none — JSON null round-trips as Some(Value::Null) server-side.
        ...(tls.value !== null ? { tls_fingerprint: tls.value } : {}),
        enabled,
      });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["providers", providerId, "credentials"] });
      toast.success(t("form.saved"));
      onSaved();
    },
    onError: (error) => {
      setFormError(error instanceof ApiError ? error.message : String(error));
    },
  });

  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        setFormError(null);
        mutation.mutate();
      }}
    >
      <div className="grid gap-2">
        <Label htmlFor="c-label">{t("fields.credLabel")}</Label>
        <Input id="c-label" value={label} onChange={(e) => setLabel(e.target.value)} />
      </div>
      <SecretEditor channel={channel} value={secretText} onChange={setSecretText} editing={editing} />
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="grid gap-2">
          <Label htmlFor="c-weight">{t("fields.weight")}</Label>
          <Input id="c-weight" inputMode="numeric" value={weight} onChange={(e) => setWeight(e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="c-rpm">{t("fields.rpm")}</Label>
          <Input id="c-rpm" inputMode="numeric" value={rpm} onChange={(e) => setRpm(e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="c-tpm">{t("fields.tpm")}</Label>
          <Input id="c-tpm" inputMode="numeric" value={tpm} onChange={(e) => setTpm(e.target.value)} />
        </div>
      </div>
      <div className="grid gap-2">
        <Label htmlFor="c-proxy">{t("fields.proxyUrl")}</Label>
        <Input id="c-proxy" value={proxyUrl} onChange={(e) => setProxyUrl(e.target.value)} />
      </div>
      <div className="grid gap-2">
        <Label htmlFor="c-tls">{t("fields.tlsFingerprint")}</Label>
        <JsonField id="c-tls" value={tlsText} onChange={setTlsText} rows={3} hint={t("form.tlsHint")} />
      </div>
      <div className="flex items-center justify-between">
        <Label htmlFor="c-enabled">{t("fields.enabled")}</Label>
        <Switch id="c-enabled" checked={enabled} onCheckedChange={setEnabled} />
      </div>
      {formError && <p className="text-sm text-destructive">{formError}</p>}
      <Button type="submit" disabled={mutation.isPending}>
        {editing ? t("creds.edit") : t("creds.add")}
      </Button>
    </form>
  );
}
