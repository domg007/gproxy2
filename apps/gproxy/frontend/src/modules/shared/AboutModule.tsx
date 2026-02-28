import { Card } from "../../components/ui";
import { useI18n } from "../../app/i18n";

export function AboutModule() {
  const { t } = useI18n();

  return (
    <Card title={t("about.title")} subtitle={t("about.subtitle")}>
      <div className="space-y-4">
        <section className="space-y-2 text-sm">
          <h3 className="font-semibold text-text">{t("about.features_title")}</h3>
          <ul className="list-disc space-y-1 pl-5 text-muted">
            <li>{t("about.feature_1")}</li>
            <li>{t("about.feature_2")}</li>
            <li>{t("about.feature_3")}</li>
            <li>{t("about.feature_4")}</li>
          </ul>
        </section>

        <section className="space-y-2 text-sm">
          <h3 className="font-semibold text-text">{t("about.vision_title")}</h3>
          <p className="leading-7 text-muted">{t("about.vision")}</p>
        </section>

        <section className="rounded-xl border border-border px-4 py-3 text-sm">
          <h3 className="font-semibold text-text">{t("about.owner_title")}</h3>
          <div className="mt-2 space-y-1 text-muted">
            <div>
              {t("about.owner_nickname_label")}:{" "}
              <span className="font-semibold text-text">{__APP_AUTHOR__}</span>
            </div>
            <div>
              {t("about.owner_email_label")}:{" "}
              <span className="font-semibold text-text">{__APP_EMAIL__}</span>
            </div>
          </div>
        </section>

        <section className="rounded-xl border border-border px-4 py-3 text-sm">
          <h3 className="font-semibold text-text">{t("about.build_title")}</h3>
          <div className="mt-2 space-y-1 text-muted">
            <div>
              {t("about.version_label")}:{" "}
              <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[12px] text-text">
                {__APP_VERSION__}
              </code>
            </div>
            <div>
              {t("about.commit_label")}:{" "}
              <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[12px] text-text">
                {__APP_COMMIT__}
              </code>
            </div>
            {__APP_HOMEPAGE__ ? (
              <div>
                {t("about.homepage_label")}:{" "}
                <a
                  className="text-text hover:underline"
                  href={__APP_HOMEPAGE__}
                  target="_blank"
                  rel="noreferrer"
                >
                  {__APP_HOMEPAGE__}
                </a>
              </div>
            ) : null}
            {__APP_REPOSITORY__ ? (
              <div>
                {t("about.repository_label")}:{" "}
                <a
                  className="text-text hover:underline"
                  href={__APP_REPOSITORY__}
                  target="_blank"
                  rel="noreferrer"
                >
                  {__APP_REPOSITORY__}
                </a>
              </div>
            ) : null}
          </div>
        </section>
      </div>
    </Card>
  );
}
