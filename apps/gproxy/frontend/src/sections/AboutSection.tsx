
import { Card } from "../components/ui";
import { useI18n } from "../i18n";

export function AboutSection() {
  const { t } = useI18n();
  const appVersion = __APP_VERSION__;
  const appCommit = __APP_COMMIT__;

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
        </section>
      </div>
    </Card>
  );
}
