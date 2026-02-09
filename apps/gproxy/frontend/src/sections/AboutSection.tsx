
import { useState } from "react";

import { Card } from "../components/ui";
import { useI18n } from "../i18n";
import { formatApiError, request } from "../lib/api";
import type { SelfUpdateResponse } from "../lib/types";

type Props = {
  adminKey: string;
  notify: (kind: "success" | "error" | "info", message: string) => void;
};

export function AboutSection({ adminKey, notify }: Props) {
  const { t } = useI18n();
  const appVersion = __APP_VERSION__;
  const appCommit = __APP_COMMIT__;
  const [updating, setUpdating] = useState(false);

  const triggerSelfUpdate = async () => {
    if (!window.confirm(t("about.self_update_confirm"))) {
      return;
    }
    setUpdating(true);
    try {
      const result = await request<SelfUpdateResponse>("/admin/system/self_update", {
        method: "POST",
        adminKey
      });
      notify(
        "success",
        t("about.self_update_ok", {
          tag: result.release_tag
        })
      );
    } catch (error) {
      notify("error", formatApiError(error));
    } finally {
      setUpdating(false);
    }
  };

  return (
    <Card title={t("about.title")} subtitle={t("about.subtitle")}>
      <div className="space-y-5 text-sm text-slate-700">
        <section>
          <h4 className="text-sm font-semibold text-slate-900">{t("about.features_title")}</h4>
          <ul className="mt-2 space-y-1.5 leading-6 text-slate-600">
            <li>{t("about.feature_1")}</li>
            <li>{t("about.feature_2")}</li>
            <li>{t("about.feature_3")}</li>
            <li>{t("about.feature_4")}</li>
          </ul>
        </section>

        <section>
          <h4 className="text-sm font-semibold text-slate-900">{t("about.vision_title")}</h4>
          <p className="mt-2 leading-7 text-slate-600">{t("about.vision")}</p>
        </section>

        <section className="rounded-xl border border-slate-200 bg-slate-50 px-4 py-3">
          <h4 className="text-sm font-semibold text-slate-900">{t("about.owner_title")}</h4>
          <div className="mt-2 text-xs leading-6 text-slate-600">
            <div>
              {t("about.owner_nickname_label")}:{" "}
              <span className="font-semibold text-slate-800">{t("about.owner_nickname")}</span>
            </div>
            <div>
              {t("about.owner_email_label")}:{" "}
              <span className="font-semibold text-slate-800">{t("about.owner_email")}</span>
            </div>
          </div>
        </section>

        <section className="rounded-xl border border-slate-200 bg-white px-4 py-3">
          <h4 className="text-sm font-semibold text-slate-900">{t("about.build_title")}</h4>
          <div className="mt-2 text-xs leading-6 text-slate-600">
            <div>
              {t("about.version_label")}:{" "}
              <code className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[11px] text-slate-800">
                {appVersion}
              </code>
            </div>
            <div>
              {t("about.commit_label")}:{" "}
              <code className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[11px] text-slate-800">
                {appCommit}
              </code>
            </div>
          </div>
          <div className="mt-3 border-t border-slate-200 pt-3">
            <div className="text-xs text-slate-500">{t("about.self_update_hint")}</div>
            <div className="mt-2">
              <button
                type="button"
                className="btn btn-neutral"
                disabled={updating}
                onClick={() => void triggerSelfUpdate()}
              >
                {updating ? t("about.self_update_running") : t("about.self_update_button")}
              </button>
            </div>
          </div>
        </section>
      </div>
    </Card>
  );
}
