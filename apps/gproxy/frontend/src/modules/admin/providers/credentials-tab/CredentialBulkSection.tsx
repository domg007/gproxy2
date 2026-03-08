import type { ChangeEvent, Dispatch, RefObject, SetStateAction } from "react";

import { Button, Label, Select, TextArea } from "../../../../components/ui";
import type { CredentialBulkMode } from "../index";
import type { TranslateFn } from "./shared";

export function CredentialBulkSection({
  bulkModes,
  bulkMode,
  setBulkMode,
  bulkInputText,
  setBulkInputText,
  bulkPlaceholder,
  bulkError,
  setBulkError,
  runBulkImport,
  runBulkExport,
  openBulkImportFilePicker,
  runBulkExportFile,
  bulkFileInputRef,
  onBulkImportFileChange,
  bulkExportText,
  t
}: {
  bulkModes: readonly CredentialBulkMode[];
  bulkMode: CredentialBulkMode;
  setBulkMode: Dispatch<SetStateAction<CredentialBulkMode>>;
  bulkInputText: string;
  setBulkInputText: Dispatch<SetStateAction<string>>;
  bulkPlaceholder: string;
  bulkError: string;
  setBulkError: Dispatch<SetStateAction<string>>;
  runBulkImport: () => void;
  runBulkExport: () => void;
  openBulkImportFilePicker: () => void;
  runBulkExportFile: () => void;
  bulkFileInputRef: RefObject<HTMLInputElement | null>;
  onBulkImportFileChange: (event: ChangeEvent<HTMLInputElement>) => void | Promise<void>;
  bulkExportText: string;
  t: TranslateFn;
}) {
  return (
    <div className="space-y-3 rounded-xl border border-border p-3">
      <div className="text-sm text-muted">{t("providers.bulk.hint")}</div>

      {bulkModes.length > 1 ? (
        <div>
          <Label>{t("providers.bulk.mode")}</Label>
          <Select
            value={bulkMode}
            onChange={(value) => {
              setBulkMode(value as CredentialBulkMode);
              setBulkError("");
            }}
            options={bulkModes.map((mode) => ({
              value: mode,
              label: t(`providers.bulk.mode.${mode}`)
            }))}
          />
        </div>
      ) : null}

      <div>
        <Label>{t("providers.bulk.input")}</Label>
        <TextArea
          rows={10}
          value={bulkInputText}
          onChange={(value) => {
            setBulkInputText(value);
            setBulkError("");
          }}
          placeholder={bulkPlaceholder}
        />
      </div>

      {bulkError ? <div className="text-sm text-red-500">{bulkError}</div> : null}

      <div className="flex flex-wrap gap-2">
        <Button onClick={runBulkImport}>{t("providers.bulk.import")}</Button>
        <Button variant="neutral" onClick={runBulkExport}>
          {t("providers.bulk.export")}
        </Button>
        <Button variant="neutral" onClick={() => setBulkInputText("")}>
          {t("providers.bulk.clearInput")}
        </Button>
        {bulkMode === "json" ? (
          <>
            <Button variant="neutral" onClick={openBulkImportFilePicker}>
              {t("providers.bulk.importFile")}
            </Button>
            <Button variant="neutral" onClick={runBulkExportFile}>
              {t("providers.bulk.exportFile")}
            </Button>
          </>
        ) : null}
      </div>

      {bulkMode === "json" ? (
        <input
          ref={bulkFileInputRef}
          type="file"
          multiple
          accept=".json,.jsonl,application/json,text/plain"
          className="hidden"
          onChange={onBulkImportFileChange}
        />
      ) : null}

      <div>
        <Label>{t("providers.bulk.exportData")}</Label>
        <TextArea
          rows={10}
          value={bulkExportText}
          onChange={() => {}}
          readOnly
          placeholder={t("providers.bulk.exportPlaceholder")}
        />
      </div>
    </div>
  );
}
