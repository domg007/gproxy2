import { useState, type Dispatch, type SetStateAction } from "react";

import { Button, Input, Label, Select, TextArea } from "../../../components/ui";
import {
  BUILD_UA_ARCH,
  BUILD_UA_OS,
  DEFAULT_GPROXY_USER_AGENT_DRAFT,
} from "./channels/shared";
import {
  CLAUDE_AGENT_SDK_PRELUDE_TEXT,
  CLAUDE_CODE_SYSTEM_PRELUDE_TEXT,
  OPERATION_OPTIONS,
  PROTOCOL_OPTIONS,
  type DispatchMode,
  type DispatchRuleDraft,
  type ProviderFormState,
  defaultChannelSettingsDraft,
  defaultDispatchRulesForChannel
} from "./index";

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export function ConfigTab({
  providerForm,
  setProviderForm,
  channelOptions,
  showCodexOAuthIssuer,
  showOAuthTriplet,
  showVertexOAuthToken,
  showClaudeCodeSettings,
  showClaudeTopLevelCacheControl,
  showCustomMaskTable,
  addDispatchRule,
  updateDispatchRule,
  removeDispatchRule,
  isCreatingProvider,
  onCancelCreate,
  onSave,
  t
}: {
  providerForm: ProviderFormState;
  setProviderForm: Dispatch<SetStateAction<ProviderFormState>>;
  channelOptions: Array<{ value: string; label: string }>;
  showCodexOAuthIssuer: boolean;
  showOAuthTriplet: boolean;
  showVertexOAuthToken: boolean;
  showClaudeCodeSettings: boolean;
  showClaudeTopLevelCacheControl: boolean;
  showCustomMaskTable: boolean;
  addDispatchRule: () => void;
  updateDispatchRule: (id: string, patch: Partial<Omit<DispatchRuleDraft, "id">>) => void;
  removeDispatchRule: (id: string) => void;
  isCreatingProvider: boolean;
  onCancelCreate: () => void;
  onSave: () => void;
  t: TranslateFn;
}) {
  const [dispatchExpanded, setDispatchExpanded] = useState(false);
  const maxDispatchRowsWhenCollapsed = 3;
  const hasMoreDispatchRules =
    providerForm.dispatchRules.length > maxDispatchRowsWhenCollapsed;
  const visibleDispatchRules =
    !dispatchExpanded && hasMoreDispatchRules
      ? providerForm.dispatchRules.slice(0, maxDispatchRowsWhenCollapsed)
      : providerForm.dispatchRules;
  const preludeTemplates = [
    {
      key: "none",
      label: t("common.none"),
      value: ""
    },
    {
      key: "code",
      label: t("providers.prelude.template.code"),
      value: CLAUDE_CODE_SYSTEM_PRELUDE_TEXT
    },
    {
      key: "agent",
      label: t("providers.prelude.template.agent"),
      value: CLAUDE_AGENT_SDK_PRELUDE_TEXT
    }
  ] as const;
  const geminiCliTemplate = `GeminiCLI/0.30.0/gemini-2.5-pro (${BUILD_UA_OS}; ${BUILD_UA_ARCH})`;
  const userAgentTemplateOptions = [
    { value: "", label: t("providers.uaTemplate.placeholder") },
    {
      value: DEFAULT_GPROXY_USER_AGENT_DRAFT,
      label: t("providers.uaTemplate.channel.gproxy")
    },
    { value: "codex_vscode/0.99.0", label: t("providers.uaTemplate.channel.codex") },
    { value: "claude-code/2.1.62", label: t("providers.uaTemplate.channel.claudecode") },
    { value: geminiCliTemplate, label: t("providers.uaTemplate.channel.geminicli") },
    {
      value: "antigravity/1.15.8 (Windows; AMD64)",
      label: t("providers.uaTemplate.channel.antigravity")
    },
    {
      value: "Visual Studio Code/1.99.0",
      label: t("providers.uaTemplate.ide.vscode")
    },
    {
      value: "IntelliJIdea/2025.3.2",
      label: t("providers.uaTemplate.ide.intellij")
    },
    { value: "PyCharm/2024.5.2", label: t("providers.uaTemplate.ide.pycharm") },
    {
      value: "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
      label: t("providers.uaTemplate.bot.googlebot")
    },
    {
      value: "Mozilla/5.0 (compatible; bingbot/2.0; +http://www.bing.com/bingbot.htm)",
      label: t("providers.uaTemplate.bot.bingbot")
    }
  ];
  const credentialPickModeOptions = [
    {
      value: "round_robin_with_cache",
      label: t("providers.pickMode.round_robin_with_cache")
    },
    {
      value: "sticky_with_cache",
      label: t("providers.pickMode.sticky_with_cache")
    },
    {
      value: "round_robin_no_cache",
      label: t("providers.pickMode.round_robin_no_cache")
    },
    {
      value: "sticky_no_cache",
      label: t("providers.pickMode.sticky_no_cache")
    }
  ];

  return (
    <div className="space-y-4">
      <div className="grid gap-3 md:grid-cols-2">
        <div>
          <Label>{t("field.id")}</Label>
          <Input value={providerForm.id} onChange={(v) => setProviderForm((p) => ({ ...p, id: v }))} />
        </div>
        <div>
          <Label>{t("field.name")}</Label>
          <Input
            value={providerForm.name}
            onChange={(v) => setProviderForm((p) => ({ ...p, name: v }))}
          />
        </div>
        <div>
          <Label>{t("field.channel")}</Label>
          <Select
            value={providerForm.channel}
            onChange={(value) =>
              setProviderForm((prev) => ({
                ...prev,
                channel: value,
                settings: defaultChannelSettingsDraft(value),
                dispatchRules: defaultDispatchRulesForChannel(value)
              }))
            }
            options={channelOptions}
          />
        </div>
        {showClaudeTopLevelCacheControl ? (
          <div>
            <Label>{t("field.enable_top_level_cache_control")}</Label>
            <Select
              value={
                (providerForm.settings.enable_top_level_cache_control ?? "false")
                  .trim()
                  .toLowerCase() === "true"
                  ? "true"
                  : "false"
              }
              onChange={(value) =>
                setProviderForm((prev) => ({
                  ...prev,
                  settings: {
                    ...prev.settings,
                    enable_top_level_cache_control: value === "true" ? "true" : "false"
                  }
                }))
              }
              options={[
                { value: "false", label: t("common.disabled") },
                { value: "true", label: t("common.enabled") }
              ]}
            />
          </div>
        ) : null}
        <div>
          <Label>{t("field.credential_pick_mode")}</Label>
          <Select
            value={providerForm.credentialPickMode}
            onChange={(value) =>
              setProviderForm((prev) => ({
                ...prev,
                credentialPickMode:
                  value === "sticky_no_cache" ||
                  value === "sticky_with_cache" ||
                  value === "round_robin_no_cache" ||
                  value === "round_robin_with_cache"
                    ? value
                    : "round_robin_with_cache"
              }))
            }
            options={credentialPickModeOptions}
          />
        </div>
        <div className="md:col-span-2">
          <Label>{t("field.base_url")}</Label>
          <Input
            value={providerForm.settings.base_url ?? ""}
            onChange={(value) =>
              setProviderForm((prev) => ({
                ...prev,
                settings: { ...prev.settings, base_url: value }
              }))
            }
          />
        </div>
        <div className="md:col-span-2 rounded-lg border border-border p-3">
          <Label>{t("field.user_agent")}</Label>
          <div className="space-y-2">
            <Select
              value=""
              onChange={(value) => {
                if (!value) {
                  return;
                }
                setProviderForm((prev) => ({
                  ...prev,
                  settings: { ...prev.settings, user_agent: value }
                }));
              }}
              options={userAgentTemplateOptions}
            />
            <Input
              value={providerForm.settings.user_agent ?? ""}
              onChange={(value) =>
                setProviderForm((prev) => ({
                  ...prev,
                  settings: { ...prev.settings, user_agent: value }
                }))
              }
            />
          </div>
        </div>
        {showCustomMaskTable ? (
          <div className="md:col-span-2 rounded-lg border border-border p-3">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
              custom
            </div>
            <Label>{t("field.custom_mask_table")}</Label>
            <TextArea
              rows={8}
              value={providerForm.settings.mask_table ?? ""}
              onChange={(value) =>
                setProviderForm((prev) => ({
                  ...prev,
                  settings: { ...prev.settings, mask_table: value }
                }))
              }
            />
            <p className="mt-2 text-xs text-muted">{t("providers.custom.maskHint")}</p>
          </div>
        ) : null}
        {showCodexOAuthIssuer ? (
          <div className="md:col-span-2 rounded-lg border border-border p-3">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
              {t("providers.section.oauth")}
            </div>
            <Label>{t("field.oauth_issuer_url")}</Label>
            <Input
              value={providerForm.settings.oauth_issuer_url ?? ""}
              onChange={(value) =>
                setProviderForm((prev) => ({
                  ...prev,
                  settings: { ...prev.settings, oauth_issuer_url: value }
                }))
              }
            />
          </div>
        ) : null}
        {showOAuthTriplet ? (
          <div className="md:col-span-2 rounded-lg border border-border p-3">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
              {t("providers.section.oauth")}
            </div>
            <div className="grid gap-3 md:grid-cols-3">
              <div>
                <Label>{t("field.oauth_authorize_url")}</Label>
                <Input
                  value={providerForm.settings.oauth_authorize_url ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, oauth_authorize_url: value }
                    }))
                  }
                />
              </div>
              <div>
                <Label>{t("field.oauth_token_url")}</Label>
                <Input
                  value={providerForm.settings.oauth_token_url ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, oauth_token_url: value }
                    }))
                  }
                />
              </div>
              <div>
                <Label>{t("field.oauth_userinfo_url")}</Label>
                <Input
                  value={providerForm.settings.oauth_userinfo_url ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, oauth_userinfo_url: value }
                    }))
                  }
                />
              </div>
            </div>
          </div>
        ) : null}
        {showVertexOAuthToken ? (
          <div className="md:col-span-2 rounded-lg border border-border p-3">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
              {t("providers.section.oauth")}
            </div>
            <Label>{t("field.oauth_token_url")}</Label>
            <Input
              value={providerForm.settings.oauth_token_url ?? ""}
              onChange={(value) =>
                setProviderForm((prev) => ({
                  ...prev,
                  settings: { ...prev.settings, oauth_token_url: value }
                }))
              }
            />
          </div>
        ) : null}
        {showClaudeCodeSettings ? (
          <div className="md:col-span-2 rounded-lg border border-border p-3">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-muted">
              claudecode
            </div>
            <div className="grid gap-3 md:grid-cols-2">
              <div>
                <Label>{t("field.claudecode_ai_base_url")}</Label>
                <Input
                  value={providerForm.settings.claudecode_ai_base_url ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, claudecode_ai_base_url: value }
                    }))
                  }
                />
              </div>
              <div>
                <Label>{t("field.claudecode_platform_base_url")}</Label>
                <Input
                  value={providerForm.settings.claudecode_platform_base_url ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, claudecode_platform_base_url: value }
                    }))
                  }
                />
              </div>
              <div className="md:col-span-2">
                <Label>{t("field.claudecode_prelude_text")}</Label>
                <TextArea
                  rows={4}
                  value={providerForm.settings.claudecode_prelude_text ?? ""}
                  onChange={(value) =>
                    setProviderForm((prev) => ({
                      ...prev,
                      settings: { ...prev.settings, claudecode_prelude_text: value }
                    }))
                  }
                />
                <div className="mt-2 flex flex-wrap gap-2">
                  {preludeTemplates.map((template) => (
                    <Button
                      key={template.key}
                      variant={
                        (providerForm.settings.claudecode_prelude_text ?? "") === template.value
                          ? "primary"
                          : "neutral"
                      }
                      onClick={() =>
                        setProviderForm((prev) => ({
                          ...prev,
                          settings: {
                            ...prev.settings,
                            claudecode_prelude_text: template.value
                          }
                        }))
                      }
                    >
                      {template.label}
                    </Button>
                  ))}
                </div>
                <p className="mt-2 text-xs text-muted">{t("providers.prelude.hint")}</p>
              </div>
            </div>
          </div>
        ) : null}
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <Label>{t("field.dispatch_rules")}</Label>
          <div className="flex items-center gap-2">
            {hasMoreDispatchRules ? (
              <Button variant="neutral" onClick={() => setDispatchExpanded((value) => !value)}>
                {dispatchExpanded
                  ? t("providers.dispatch.collapse")
                  : t("providers.dispatch.expand")}
              </Button>
            ) : null}
            <Button variant="neutral" onClick={addDispatchRule}>
              {t("providers.addRule")}
            </Button>
          </div>
        </div>
        <div className="space-y-3">
          {visibleDispatchRules.map((rule) => (
            <div key={rule.id} className="provider-card space-y-2">
              <div className="grid gap-2 md:grid-cols-6">
                <div>
                  <Label>{t("field.src_op")}</Label>
                  <Select
                    value={rule.srcOperation}
                    onChange={(value) => updateDispatchRule(rule.id, { srcOperation: value })}
                    options={OPERATION_OPTIONS.map((item) => ({ value: item, label: item }))}
                  />
                </div>
                <div>
                  <Label>{t("field.src_proto")}</Label>
                  <Select
                    value={rule.srcProtocol}
                    onChange={(value) => updateDispatchRule(rule.id, { srcProtocol: value })}
                    options={PROTOCOL_OPTIONS.map((item) => ({ value: item, label: item }))}
                  />
                </div>
                <div>
                  <Label>{t("field.mode")}</Label>
                  <Select
                    value={rule.mode}
                    onChange={(value) =>
                      updateDispatchRule(rule.id, { mode: value as DispatchMode })
                    }
                    options={[
                      { value: "passthrough", label: t("providers.mode.passthrough") },
                      { value: "transform", label: t("providers.mode.transform") },
                      { value: "local", label: t("providers.mode.local") },
                      { value: "unsupported", label: t("providers.mode.unsupported") }
                    ]}
                  />
                </div>
                <div>
                  <Label>{t("field.dst_op")}</Label>
                  <Select
                    value={rule.dstOperation}
                    onChange={(value) => updateDispatchRule(rule.id, { dstOperation: value })}
                    options={OPERATION_OPTIONS.map((item) => ({ value: item, label: item }))}
                    disabled={rule.mode !== "transform"}
                  />
                </div>
                <div>
                  <Label>{t("field.dst_proto")}</Label>
                  <Select
                    value={rule.dstProtocol}
                    onChange={(value) => updateDispatchRule(rule.id, { dstProtocol: value })}
                    options={PROTOCOL_OPTIONS.map((item) => ({ value: item, label: item }))}
                    disabled={rule.mode !== "transform"}
                  />
                </div>
                <div className="flex items-end">
                  <Button variant="danger" onClick={() => removeDispatchRule(rule.id)}>
                    {t("common.delete")}
                  </Button>
                </div>
              </div>
            </div>
          ))}
          {providerForm.dispatchRules.length > visibleDispatchRules.length ? (
            <p className="text-xs text-muted">
              {t("providers.dispatch.visibleHint", {
                shown: visibleDispatchRules.length,
                total: providerForm.dispatchRules.length
              })}
            </p>
          ) : null}
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button onClick={onSave}>{t("providers.save")}</Button>
        {isCreatingProvider ? (
          <Button variant="neutral" onClick={onCancelCreate}>
            {t("common.cancel")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
