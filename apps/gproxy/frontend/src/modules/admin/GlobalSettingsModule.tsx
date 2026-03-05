import { useEffect, useRef, useState, type ChangeEvent } from "react";

import { useI18n } from "../../app/i18n";
import { apiRequest, formatError } from "../../lib/api";
import type { GlobalSettingsRow } from "../../lib/types";
import { Button, Card, Input, Label, Select } from "../../components/ui";

const DEFAULT_HF_URL = "https://huggingface.co";
const DEFAULT_SPOOF_EMULATION = "chrome_136";
const DEFAULT_UPDATE_SOURCE = "github";
const DEFAULT_UPDATE_CHANNEL = "releases";
const UPDATE_CHANNEL_STORAGE_KEY = "gproxy_update_channel";
const SPOOF_EMULATION_OPTIONS = [
  { value: "chrome_136", label: "Chrome 136" },
  { value: "chrome_137", label: "Chrome 137" },
  { value: "chrome_138", label: "Chrome 138" },
  { value: "edge_136", label: "Edge 136" },
  { value: "edge_137", label: "Edge 137" },
  { value: "firefox_136", label: "Firefox 136" },
  { value: "firefox_139", label: "Firefox 139" },
  { value: "safari_18_5", label: "Safari 18.5" }
];
const UPDATE_SOURCE_OPTIONS = [
  { value: "github", labelKey: "global.updateSource.github" },
  { value: "cnb", labelKey: "global.updateSource.cnb" }
] as const;
const UPDATE_CHANNEL_OPTIONS = [
  { value: "releases", labelKey: "global.updateChannel.releases" },
  { value: "staging", labelKey: "global.updateChannel.staging" }
] as const;

function normalizeUpdateChannel(value: string | null | undefined): string {
  const normalized = (value ?? "").trim().toLowerCase();
  return normalized === "staging" ? "staging" : DEFAULT_UPDATE_CHANNEL;
}

function normalizeUpdateSource(value: string | null | undefined): string {
  const normalized = (value ?? "").trim().toLowerCase();
  return normalized === "cnb" ? "cnb" : DEFAULT_UPDATE_SOURCE;
}

function readStoredUpdateChannel(): string {
  if (typeof window === "undefined") {
    return DEFAULT_UPDATE_CHANNEL;
  }
  return normalizeUpdateChannel(window.localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY));
}

function persistUpdateChannel(value: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(UPDATE_CHANNEL_STORAGE_KEY, normalizeUpdateChannel(value));
}

export function GlobalSettingsModule({
  apiKey,
  notify
}: {
  apiKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
}) {
  const { t } = useI18n();
  const [loading, setLoading] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [selfUpdating, setSelfUpdating] = useState(false);
  const importInputRef = useRef<HTMLInputElement | null>(null);
  const [form, setForm] = useState({
    host: "",
    port: "8787",
    hfToken: "",
    hfUrl: DEFAULT_HF_URL,
    proxy: "",
    spoofEmulation: DEFAULT_SPOOF_EMULATION,
    updateSource: DEFAULT_UPDATE_SOURCE,
    updateChannel: readStoredUpdateChannel(),
    adminKey: "",
    dsn: "",
    dataDir: "",
    maskSensitiveInfo: true
  });

  const load = async () => {
    setLoading(true);
    try {
      const row = await apiRequest<GlobalSettingsRow | null>("/admin/global-settings", {
        apiKey,
        method: "GET"
      });
      if (row) {
        setForm({
          host: row.host,
          port: String(row.port),
          hfToken: row.hf_token ?? "",
          hfUrl: row.hf_url ?? DEFAULT_HF_URL,
          proxy: row.proxy ?? "",
          spoofEmulation: row.spoof_emulation ?? DEFAULT_SPOOF_EMULATION,
          updateSource: normalizeUpdateSource(row.update_source),
          updateChannel: readStoredUpdateChannel(),
          adminKey: row.admin_key,
          dsn: row.dsn,
          dataDir: row.data_dir,
          maskSensitiveInfo: row.mask_sensitive_info
        });
      }
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const save = async () => {
    try {
      persistUpdateChannel(form.updateChannel);
      await apiRequest("/admin/global-settings/upsert", {
        apiKey,
        method: "POST",
        body: {
          host: form.host.trim(),
          port: Number(form.port),
          hf_token: form.hfToken.trim() || null,
          hf_url: form.hfUrl.trim() || null,
          proxy: form.proxy.trim() || null,
          spoof_emulation: form.spoofEmulation,
          update_source: normalizeUpdateSource(form.updateSource),
          admin_key: form.adminKey.trim(),
          mask_sensitive_info: form.maskSensitiveInfo,
          dsn: form.dsn.trim(),
          data_dir: form.dataDir.trim()
        }
      });
      notify("success", t("global.saved"));
    } catch (error) {
      notify("error", formatError(error));
    }
  };

  const exportToml = async () => {
    setExporting(true);
    try {
      const res = await fetch("/admin/config/export-toml", {
        method: "GET",
        headers: {
          "x-api-key": apiKey
        }
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `HTTP ${res.status}`);
      }

      const blob = await res.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const disposition = res.headers.get("content-disposition") ?? "";
      const match = disposition.match(/filename="?([^"]+)"?/i);
      const filename = match?.[1]?.trim() || "gproxy.toml";

      const anchor = document.createElement("a");
      anchor.href = downloadUrl;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(downloadUrl);

      notify("success", t("global.exported"));
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setExporting(false);
    }
  };

  const importTomlText = async (text: string) => {
    const content = text.trim();
    if (!content) {
      notify("error", t("global.importEmpty"));
      return;
    }
    setImporting(true);
    try {
      await apiRequest("/admin/config/import-toml", {
        apiKey,
        method: "POST",
        body: {
          toml: content
        }
      });
      notify("success", t("global.imported"));
      await load();
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setImporting(false);
    }
  };

  const onPickImportFile = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) {
      return;
    }
    try {
      const text = await file.text();
      await importTomlText(text);
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      event.target.value = "";
    }
  };

  const triggerSelfUpdate = async () => {
    if (!window.confirm(t("global.selfUpdateConfirm"))) {
      return;
    }
    setSelfUpdating(true);
    try {
      const updateChannel = normalizeUpdateChannel(form.updateChannel);
      persistUpdateChannel(updateChannel);
      const result = await apiRequest<{ release_tag?: string; tag?: string }>(
        `/admin/system/self_update?update_channel=${encodeURIComponent(updateChannel)}`,
        {
        apiKey,
        method: "POST"
      });
      const tag = result.release_tag ?? result.tag ?? "latest";
      notify("success", t("global.selfUpdateOk", { tag }));
    } catch (error) {
      notify("error", formatError(error));
    } finally {
      setSelfUpdating(false);
    }
  };

  return (
    <Card
      title={t("global.title")}
      action={
        <div className="flex gap-2">
          <input
            ref={importInputRef}
            type="file"
            accept=".toml,text/plain"
            className="hidden"
            onChange={(event) => void onPickImportFile(event)}
          />
          <Button
            variant="secondary"
            onClick={() => importInputRef.current?.click()}
            disabled={importing}
          >
            {importing ? t("global.importing") : t("global.import")}
          </Button>
          <Button variant="secondary" onClick={() => void exportToml()} disabled={exporting}>
            {exporting ? t("global.exporting") : t("global.export")}
          </Button>
          <Button variant="secondary" onClick={() => void load()} disabled={loading}>
            {loading ? t("global.refreshing") : t("common.refresh")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => void triggerSelfUpdate()}
            disabled={selfUpdating}
          >
            {selfUpdating ? t("global.selfUpdateRunning") : t("global.selfUpdateButton")}
          </Button>
        </div>
      }
    >
      <div className="grid gap-3 md:grid-cols-2">
        <div>
          <Label>{t("field.host")}</Label>
          <Input value={form.host} onChange={(v) => setForm((p) => ({ ...p, host: v }))} />
        </div>
        <div>
          <Label>{t("field.port")}</Label>
          <Input
            type="number"
            value={form.port}
            onChange={(v) => setForm((p) => ({ ...p, port: v }))}
          />
        </div>
        <div>
          <Label>{t("field.proxy")}</Label>
          <Input value={form.proxy} onChange={(v) => setForm((p) => ({ ...p, proxy: v }))} />
        </div>
        <div>
          <Label>{t("field.spoof_emulation")}</Label>
          <Select
            value={form.spoofEmulation}
            onChange={(v) => setForm((p) => ({ ...p, spoofEmulation: v }))}
            options={SPOOF_EMULATION_OPTIONS}
          />
        </div>
        <div>
          <Label>{t("field.update_source")}</Label>
          <Select
            value={form.updateSource}
            onChange={(v) =>
              setForm((p) => ({ ...p, updateSource: normalizeUpdateSource(v) }))
            }
            options={UPDATE_SOURCE_OPTIONS.map((item) => ({
              value: item.value,
              label: t(item.labelKey)
            }))}
          />
        </div>
        <div>
          <Label>{t("field.update_channel")}</Label>
          <Select
            value={form.updateChannel}
            onChange={(v) => {
              const next = normalizeUpdateChannel(v);
              setForm((p) => ({ ...p, updateChannel: next }));
              persistUpdateChannel(next);
            }}
            options={UPDATE_CHANNEL_OPTIONS.map((item) => ({
              value: item.value,
              label: t(item.labelKey)
            }))}
          />
        </div>
        <div>
          <Label>{t("field.hf_token")}</Label>
          <Input
            type="password"
            value={form.hfToken}
            onChange={(v) => setForm((p) => ({ ...p, hfToken: v }))}
          />
        </div>
        <div>
          <Label>{t("field.hf_url")}</Label>
          <Input value={form.hfUrl} onChange={(v) => setForm((p) => ({ ...p, hfUrl: v }))} />
        </div>
        <div>
          <Label>{t("field.admin_key")}</Label>
          <Input
            type="password"
            value={form.adminKey}
            onChange={(v) => setForm((p) => ({ ...p, adminKey: v }))}
          />
        </div>
        <div>
          <Label>{t("field.dsn")}</Label>
          <Input value={form.dsn} onChange={(v) => setForm((p) => ({ ...p, dsn: v }))} />
        </div>
        <div>
          <Label>{t("field.data_dir")}</Label>
          <Input value={form.dataDir} onChange={(v) => setForm((p) => ({ ...p, dataDir: v }))} />
        </div>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <input
          id="mask-sensitive"
          type="checkbox"
          checked={form.maskSensitiveInfo}
          onChange={(e) => setForm((p) => ({ ...p, maskSensitiveInfo: e.target.checked }))}
        />
        <label htmlFor="mask-sensitive" className="text-sm text-muted">
          {t("global.maskSensitive")}
        </label>
      </div>
      <div className="mt-4">
        <Button onClick={() => void save()}>{t("common.save")}</Button>
      </div>
    </Card>
  );
}
