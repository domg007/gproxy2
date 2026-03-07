import { useCallback, useMemo } from "react";

import { LoginView } from "../components/LoginView";
import { Nav } from "../components/Nav";
import { Toast } from "../components/Toast";
import { Button } from "../components/ui";
import { useI18n } from "./i18n";
import {
  buildAdminNavItems,
  buildUserNavItems,
  defaultModule,
  renderActiveModule
} from "./modules";
import { useActiveModule } from "./hooks/useActiveModule";
import { useAdminReleaseCheck } from "./hooks/useAdminReleaseCheck";
import { useAppSession } from "./hooks/useAppSession";
import { useThemeFab } from "./hooks/useThemeFab";
import { useTimedToast } from "./hooks/useTimedToast";

export function App() {
  const { locale, setLocale, t } = useI18n();
  const appVersion = useMemo(() => __APP_VERSION__.trim() || "dev", []);
  const appCommit = useMemo(() => {
    const commit = __APP_COMMIT__.trim();
    if (!commit || commit === "unknown") {
      return "unknown";
    }
    return commit.slice(0, 8);
  }, []);
  const { toast, notify } = useTimedToast();
  const { apiKey, role, loginLoading, restoringSession, login, logout } = useAppSession({ t });
  const adminNavItems = useMemo(() => buildAdminNavItems(t), [t]);
  const userNavItems = useMemo(() => buildUserNavItems(t), [t]);

  const navItems = useCallback(
    (currentRole: "admin" | "user") =>
      currentRole === "admin" ? adminNavItems : userNavItems,
    [adminNavItems, userNavItems]
  );

  const isValidModule = useCallback(
    (currentRole: "admin" | "user", moduleId: string) =>
      navItems(currentRole).some((item) => item.id === moduleId),
    [navItems]
  );

  const { activeModule, selectModule, initializeModule, resetModule } = useActiveModule({
    role,
    isValidModule,
    defaultModule
  });

  const {
    isDarkTheme,
    themeFabPosition,
    onThemeFabPointerCancel,
    onThemeFabPointerDown,
    onThemeFabPointerMove,
    onThemeFabPointerUp
  } = useThemeFab();

  useAdminReleaseCheck({
    apiKey,
    role,
    appVersion,
    notify,
    t
  });

  const onLogin = useCallback(
    async (name: string, password: string) => {
      const session = await login(name, password);
      initializeModule(session.role);
      notify("success", t("app.loginAs", { role: session.role }));
    },
    [initializeModule, login, notify, t]
  );

  const onLogout = useCallback(() => {
    logout();
    resetModule();
    notify("info", t("app.loggedOut"));
  }, [logout, notify, resetModule, t]);

  const content = useMemo(
    () => renderActiveModule({ role, activeModule, apiKey, notify }),
    [activeModule, apiKey, notify, role]
  );

  if (restoringSession) {
    return (
      <div className="loading-shell">
        <p className="text-sm text-muted">{t("app.restoring")}</p>
      </div>
    );
  }

  if (!apiKey || !role) {
    return (
      <div className="app-shell">
        <LoginView onLogin={onLogin} loading={loginLoading} />
        <Toast toast={toast} />
      </div>
    );
  }

  return (
    <div className="app-shell">
      <header className="topbar-shell">
        <div className="topbar-panel mx-auto flex w-full max-w-[1700px] flex-col gap-3 px-4 py-3 md:flex-row md:items-center md:justify-between md:gap-4">
          <div className="flex min-w-0 flex-wrap items-center gap-2 md:gap-3">
            <h1 className="topbar-title">{t("app.title")}</h1>
            <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[11px] text-muted">
              v{appVersion}
            </code>
            <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[11px] text-muted">
              {appCommit}
            </code>
          </div>
          <div className="flex w-full items-center justify-between gap-2 md:w-auto md:justify-end md:gap-3">
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="topbar-locale-toggle topbar-segmented"
                onClick={() => setLocale(locale === "zh" ? "en" : "zh")}
                aria-label={t("app.locale.switcher")}
                title={t("app.locale.switcher")}
              >
                <span
                  className={`topbar-segmented-item ${locale === "zh" ? "topbar-segmented-item-active" : ""}`}
                >
                  {t("app.locale.short.zh")}
                </span>
                <span
                  className={`topbar-segmented-item ${locale === "en" ? "topbar-segmented-item-active" : ""}`}
                >
                  {t("app.locale.short.en")}
                </span>
              </button>
            </div>
            <Button variant="neutral" onClick={onLogout}>
              {t("app.logout")}
            </Button>
          </div>
        </div>
      </header>

      <main className="layout-shell">
        <Nav
          items={navItems(role)}
          active={activeModule}
          onChange={(moduleId) => selectModule(role, moduleId)}
        />
        <section className="content-shell">{content}</section>
      </main>
      <button
        type="button"
        className="theme-fab"
        style={{ left: themeFabPosition.x, top: themeFabPosition.y }}
        onPointerDown={onThemeFabPointerDown}
        onPointerMove={onThemeFabPointerMove}
        onPointerUp={onThemeFabPointerUp}
        onPointerCancel={onThemeFabPointerCancel}
        aria-label={t("app.theme.toggle")}
        title={t("app.theme.toggle")}
      >
        {isDarkTheme ? (
          <svg viewBox="0 0 24 24" className="theme-fab-icon" aria-hidden="true">
            <path
              fill="currentColor"
              d="M21.64 13a1 1 0 0 0-1.06-.57 8 8 0 0 1-9-9 1 1 0 0 0-1.63-.93A10 10 0 1 0 22.5 14.67a1 1 0 0 0-.86-1.67z"
            />
          </svg>
        ) : (
          <svg
            viewBox="0 0 24 24"
            className="theme-fab-icon"
            aria-hidden="true"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.9"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3.6" fill="currentColor" stroke="none" />
            <path d="M12 2.5v2.5M12 19v2.5M4.22 4.22l1.77 1.77M18.01 18.01l1.77 1.77M2.5 12H5M19 12h2.5M4.22 19.78l1.77-1.77M18.01 5.99l1.77-1.77" />
          </svg>
        )}
      </button>
      <Toast toast={toast} />
    </div>
  );
}
