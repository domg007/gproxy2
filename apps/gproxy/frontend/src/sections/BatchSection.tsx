import { useEffect, useMemo, useState } from "react";

import { request, formatApiError } from "../lib/api";
import type { ProviderDetail } from "../lib/types";
import { kindFromConfig } from "../lib/provider_schema";
import {
  buildImportedCredentialFromJson,
  buildImportedCredentialFromKey,
  isJsonCredentialKind,
  parseJsonCredentialText
} from "../lib/credential_import";
import { Button, Card, FieldLabel, TextArea } from "../components/ui";
import { useI18n } from "../i18n";

type Props = {
  adminKey: string;
  providers: ProviderDetail[];
  notify: (kind: "success" | "error" | "info", message: string) => void;
  onImported: () => void;
};

export function BatchSection({ adminKey, providers, notify, onImported }: Props) {
  const { t } = useI18n();
  const [providerName, setProviderName] = useState(providers[0]?.name ?? "");
  const [importText, setImportText] = useState("");
  const [files, setFiles] = useState<File[]>([]);

  const provider = useMemo(
    () => providers.find((item) => item.name === providerName) ?? null,
    [providerName, providers]
  );
  const providerKind = provider ? kindFromConfig(provider.config_json) : null;
  const sessionKeyLineKind = providerKind === "claudecode";
  const jsonCredentialKind = providerKind
    ? isJsonCredentialKind(providerKind) && !sessionKeyLineKind
    : false;

  useEffect(() => {
    if (!providerName && providers.length > 0) {
      setProviderName(providers[0].name);
    }
  }, [providerName, providers]);

  useEffect(() => {
    setImportText("");
    setFiles([]);
  }, [providerKind]);

  const runImport = async () => {
    if (!provider || !providerKind) {
      notify("error", t("errors.missing_provider"));
      return;
    }
    try {
      const payloads: Array<{ name: string | null; secretJson: Record<string, unknown> }> = [];

      if (sessionKeyLineKind) {
        if (importText.trim()) {
          const lines = importText
            .split(/\r?\n/)
            .map((line) => line.trim())
            .filter(Boolean);
          for (const line of lines) {
            payloads.push(buildImportedCredentialFromKey(providerKind, line));
          }
        }
      } else if (jsonCredentialKind) {
        if (importText.trim()) {
          const parsed = parseJsonCredentialText(importText);
          for (const item of parsed) {
            const payload = buildImportedCredentialFromJson(providerKind, item);
            if (payload) {
              payloads.push(payload);
            }
          }
        }

        for (const file of files) {
          const parsed = parseJsonCredentialText(await file.text());
          for (const item of parsed) {
            const payload = buildImportedCredentialFromJson(providerKind, item);
            if (payload) {
              payloads.push(payload);
            }
          }
        }
      } else if (importText.trim()) {
        const lines = importText
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean);
        for (const line of lines) {
          payloads.push(buildImportedCredentialFromKey(providerKind, line));
        }
      }

      if (payloads.length === 0) {
        notify("info", t("common.empty"));
        return;
      }

      for (const payload of payloads) {
        await request(`/admin/providers/${provider.name}/credentials`, {
          method: "POST",
          adminKey,
          body: {
            name: payload.name,
            settings_json: {},
            secret_json: payload.secretJson,
            enabled: true
          }
        });
      }

      setImportText("");
      setFiles([]);
      notify("success", `${t("credentials.import_ok")} (${payloads.length})`);
      onImported();
    } catch (error) {
      if (error instanceof Error && error.message.includes("Invalid JSON document")) {
        notify("error", t("errors.invalid_json"));
        return;
      }
      notify("error", formatApiError(error));
    }
  };

  return (
    <Card title={t("nav.batch")} subtitle={t("credentials.import_hint")}>
      <div className="grid gap-4 md:grid-cols-2">
        <div>
          <FieldLabel>{t("common.provider")}</FieldLabel>
          <select className="mt-2 select" value={providerName} onChange={(event) => setProviderName(event.target.value)}>
            {providers.map((item) => (
              <option key={item.name} value={item.name}>
                {item.name}
              </option>
            ))}
          </select>
          <div className="mt-2 text-xs text-slate-500">
            {sessionKeyLineKind
              ? t("credentials.import_mode_session_key")
              : jsonCredentialKind
                ? t("credentials.import_mode_json")
                : t("credentials.import_mode_key")}
          </div>
        </div>
        {jsonCredentialKind ? (
          <div>
            <FieldLabel>{t("credentials.import_files")}</FieldLabel>
            <div className="mt-2 space-y-2">
              <input
                type="file"
                multiple
                accept="application/json,.json"
                onChange={(event) => setFiles(Array.from(event.target.files ?? []))}
              />
              <div className="text-xs text-slate-500">{t("credentials.import_files_count", { count: String(files.length) })}</div>
            </div>
          </div>
        ) : null}
        <div className="md:col-span-2">
          <FieldLabel>
            {sessionKeyLineKind
              ? t("credentials.import_session_keys")
              : jsonCredentialKind
                ? t("credentials.import_json_text")
                : t("credentials.import_keys")}
          </FieldLabel>
          <div className="mt-2">
            <TextArea
              value={importText}
              onChange={setImportText}
              rows={8}
              placeholder={
                sessionKeyLineKind
                  ? t("credentials.import_session_keys_placeholder")
                  : jsonCredentialKind
                  ? t("credentials.import_json_placeholder")
                  : t("credentials.import_keys_placeholder")
              }
            />
          </div>
        </div>
      </div>
      <div className="mt-4">
        <Button onClick={() => void runImport()}>{t("credentials.import_run")}</Button>
      </div>
    </Card>
  );
}
