import type { Dispatch, ReactNode, SetStateAction } from "react";

import { Button, Input, Label, TextArea } from "../../../../components/ui";
import type {
  ChannelOAuthCallbackButton,
  ChannelOAuthStartButton,
  ChannelOAuthUi
} from "../channels/oauth";
import type {
  ChannelCredentialSchema,
  CredentialFieldSchema,
  CredentialFormState
} from "../index";
import type { TranslateFn } from "./shared";

export function CredentialSingleSection({
  credentialForm,
  setCredentialForm,
  credentialSchema,
  renderCredentialField,
  onUpsertCredential,
  supportsOAuth,
  oauthUi,
  oauthStartButtons,
  oauthCallbackButtons,
  oauthCallbackUsesCustomFields,
  oauthStartQuery,
  oauthCallbackQuery,
  oauthRawResult,
  oauthOpenUrl,
  extraSectionBeforeOAuth,
  renderOAuthField,
  onRunCredentialOAuthStart,
  onRunCredentialOAuthCallback,
  t
}: {
  credentialForm: CredentialFormState;
  setCredentialForm: Dispatch<SetStateAction<CredentialFormState>>;
  credentialSchema: ChannelCredentialSchema;
  renderCredentialField: (field: CredentialFieldSchema) => ReactNode;
  onUpsertCredential: () => void;
  supportsOAuth: boolean;
  oauthUi?: ChannelOAuthUi;
  oauthStartButtons: readonly ChannelOAuthStartButton[];
  oauthCallbackButtons: readonly ChannelOAuthCallbackButton[];
  oauthCallbackUsesCustomFields: boolean;
  oauthStartQuery: string;
  oauthCallbackQuery: string;
  oauthRawResult: string;
  oauthOpenUrl?: string;
  extraSectionBeforeOAuth?: ReactNode;
  renderOAuthField: (
    kind: "start" | "callback",
    field: string,
    rawQuery: string
  ) => ReactNode;
  onRunCredentialOAuthStart: (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => void;
  onRunCredentialOAuthCallback: (
    credentialId?: number,
    mode?: string,
    queryDefaults?: Record<string, string | null | undefined>
  ) => void;
  t: TranslateFn;
}) {
  return (
    <>
      <div className="grid gap-3 md:grid-cols-2">
        <div>
          <Label>{t("field.id")}</Label>
          <Input value={credentialForm.id} onChange={(v) => setCredentialForm((p) => ({ ...p, id: v }))} />
        </div>
        <div>
          <Label>{t("field.nameOptional")}</Label>
          <Input
            value={credentialForm.name}
            onChange={(v) => setCredentialForm((p) => ({ ...p, name: v }))}
          />
        </div>
        {credentialSchema.fields.map((field) => renderCredentialField(field))}
      </div>

      <div>
        <Button onClick={onUpsertCredential}>{t("common.save")}</Button>
      </div>

      {extraSectionBeforeOAuth ?? null}

      {supportsOAuth ? (
        <div className="provider-card space-y-2">
          <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
            {t("providers.section.oauth")}
          </div>
          {oauthUi?.startFields.map((field) => renderOAuthField("start", field, oauthStartQuery))}
          <div className="flex flex-wrap gap-2">
            {oauthStartButtons.map((button) => (
              <Button
                key={button.labelKey}
                variant={button.mode ? "neutral" : "primary"}
                onClick={() => onRunCredentialOAuthStart(undefined, button.mode, button.queryDefaults)}
              >
                {t(button.labelKey)}
              </Button>
            ))}
            {oauthOpenUrl ? (
              <a
                className="btn btn-primary inline-flex"
                href={oauthOpenUrl}
                target="_blank"
                rel="noopener noreferrer"
              >
                {t("providers.oauth.openAuthUrl")}
              </a>
            ) : null}
            {!oauthCallbackUsesCustomFields
              ? oauthCallbackButtons.map((button) => (
                  <Button
                    key={button.labelKey}
                    variant={button.mode ? "neutral" : "primary"}
                    onClick={() =>
                      onRunCredentialOAuthCallback(undefined, button.mode, button.queryDefaults)
                    }
                  >
                    {t(button.labelKey)}
                  </Button>
                ))
              : null}
          </div>
          {!oauthCallbackUsesCustomFields
            ? oauthUi?.callbackFields.map((field) =>
                renderOAuthField("callback", field, oauthCallbackQuery)
              )
            : null}
          {oauthCallbackUsesCustomFields ? (
            <div className="space-y-2">
              {oauthCallbackButtons.map((button) => {
                const fields = button.fields ?? oauthUi?.callbackFields ?? [];
                return (
                  <div
                    key={`callback-${button.labelKey}`}
                    className="space-y-2 rounded-lg border border-border p-3"
                  >
                    <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                      {t(button.labelKey)}
                    </div>
                    {fields.map((field) => renderOAuthField("callback", field, oauthCallbackQuery))}
                    <Button
                      variant={button.mode ? "neutral" : "primary"}
                      onClick={() =>
                        onRunCredentialOAuthCallback(undefined, button.mode, button.queryDefaults)
                      }
                    >
                      {t(button.labelKey)}
                    </Button>
                  </div>
                );
              })}
            </div>
          ) : null}
          {oauthRawResult ? (
            <div className="space-y-2 rounded-lg border border-border p-3">
              <div className="text-xs font-semibold uppercase tracking-[0.08em] text-muted">
                {t("providers.oauth.response")}
              </div>
              <TextArea value={oauthRawResult} rows={10} readOnly onChange={() => {}} />
            </div>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
