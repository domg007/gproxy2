import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { LoginView } from "../components/LoginView";
import { Nav, type NavItem } from "../components/Nav";
import { Toast, type ToastState } from "../components/Toast";
import { Badge, Button } from "../components/ui";
import { apiRequest } from "../lib/api";
import type { ThemeMode, UserRole } from "../lib/types";
import { detectRole } from "./session";
import { applyTheme, persistTheme, readStoredTheme } from "./theme";
import { GlobalSettingsModule } from "../modules/admin/GlobalSettingsModule";
import { ProvidersModule } from "../modules/admin/ProvidersModule";
import { RequestsModule } from "../modules/admin/RequestsModule";
import { UsageModule } from "../modules/admin/UsageModule";
import { UsersModule } from "../modules/admin/UsersModule";
import { MyKeysModule } from "../modules/user/MyKeysModule";
import { MyUsageModule } from "../modules/user/MyUsageModule";
import { AboutModule } from "../modules/shared/AboutModule";
import { useI18n } from "./i18n";

const API_KEY_STORAGE_KEY = "gproxy_api_key";
const ROLE_STORAGE_KEY = "gproxy_role";

const ADMIN_NAV_IDS = [
  "global-settings",
  "providers",
  "users",
  "requests",
  "usage",
  "about"
] as const;
const USER_NAV_IDS = ["my-keys", "my-usage", "about"] as const;

type AdminNavId = (typeof ADMIN_NAV_IDS)[number];
type UserNavId = (typeof USER_NAV_IDS)[number];

type LoginResponse = {
  user_id: number;
  api_key: string;
};

function defaultModule(role: UserRole): string {
  return role === "admin" ? "providers" : USER_NAV_IDS[0];
}

function parseHashRoute(hash: string): { role: UserRole; module: string } | null {
  const trimmed = hash.startsWith("#") ? hash.slice(1) : hash;
  const segments = trimmed.split("/").filter(Boolean);
  if (segments.length < 2) {
    return null;
  }
  const [roleRaw, module] = segments;
  if (roleRaw !== "admin" && roleRaw !== "user") {
    return null;
  }
  if (!module) {
    return null;
  }
  return { role: roleRaw, module };
}

function setHashRoute(role: UserRole, moduleId: string): void {
  const next = `#/${role}/${moduleId}`;
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}

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

  const [apiKey, setApiKey] = useState<string | null>(null);
  const [role, setRole] = useState<UserRole | null>(null);
  const [activeModule, setActiveModule] = useState<string>("");
  const [loginLoading, setLoginLoading] = useState(false);
  const [restoringSession, setRestoringSession] = useState(true);
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readStoredTheme());
  const [toast, setToast] = useState<ToastState | null>(null);
  const toastTimer = useRef<number | null>(null);

  const adminNavItems = useMemo<NavItem[]>(
    () => [
      { id: "global-settings", label: t("app.nav.globalSettings") },
      { id: "providers", label: t("app.nav.providers") },
      { id: "users", label: t("app.nav.users") },
      { id: "requests", label: t("app.nav.requests") },
      { id: "usage", label: t("app.nav.usage") },
      { id: "about", label: t("app.nav.about") }
    ],
    [t]
  );

  const userNavItems = useMemo<NavItem[]>(
    () => [
      { id: "my-keys", label: t("app.nav.myKeys") },
      { id: "my-usage", label: t("app.nav.myUsage") },
      { id: "about", label: t("app.nav.about") }
    ],
    [t]
  );

  const navItems = useCallback(
    (currentRole: UserRole): NavItem[] => (currentRole === "admin" ? adminNavItems : userNavItems),
    [adminNavItems, userNavItems]
  );

  const isValidModule = useCallback(
    (currentRole: UserRole, moduleId: string): boolean =>
      navItems(currentRole).some((item) => item.id === moduleId),
    [navItems]
  );

  const notify = useCallback((kind: ToastState["kind"], message: string) => {
    if (toastTimer.current !== null) {
      window.clearTimeout(toastTimer.current);
    }
    setToast({ kind, message });
    toastTimer.current = window.setTimeout(() => {
      setToast(null);
      toastTimer.current = null;
    }, 2600);
  }, []);

  useEffect(() => {
    applyTheme(themeMode);
    persistTheme(themeMode);
  }, [themeMode]);

  useEffect(() => {
    if (themeMode !== "system") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [themeMode]);

  useEffect(
    () => () => {
      if (toastTimer.current !== null) {
        window.clearTimeout(toastTimer.current);
      }
    },
    []
  );

  useEffect(() => {
    let active = true;

    const restore = async () => {
      const storedApiKey = localStorage.getItem(API_KEY_STORAGE_KEY)?.trim();
      if (!storedApiKey) {
        setRestoringSession(false);
        return;
      }
      try {
        const session = await detectRole(storedApiKey);
        if (!active) {
          return;
        }
        setApiKey(storedApiKey);
        setRole(session.role);
        localStorage.setItem(ROLE_STORAGE_KEY, session.role);
      } catch {
        localStorage.removeItem(API_KEY_STORAGE_KEY);
        localStorage.removeItem(ROLE_STORAGE_KEY);
      } finally {
        if (active) {
          setRestoringSession(false);
        }
      }
    };

    void restore();
    return () => {
      active = false;
    };
  }, []);

  const syncModuleWithHash = useCallback(
    (currentRole: UserRole) => {
      const parsed = parseHashRoute(window.location.hash);
      if (parsed && parsed.role === currentRole && isValidModule(currentRole, parsed.module)) {
        setActiveModule(parsed.module);
        return;
      }
      const fallback = defaultModule(currentRole);
      setActiveModule(fallback);
      setHashRoute(currentRole, fallback);
    },
    [isValidModule]
  );

  useEffect(() => {
    if (!role) {
      return;
    }
    const onHashChange = () => syncModuleWithHash(role);
    onHashChange();
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [role, syncModuleWithHash]);

  const onLogin = useCallback(
    async (name: string, password: string) => {
      const userName = name.trim();
      if (!userName) {
        throw new Error(t("app.error.usernameEmpty"));
      }
      if (!password.trim()) {
        throw new Error(t("app.error.passwordEmpty"));
      }
      setLoginLoading(true);
      try {
        const login = await apiRequest<LoginResponse>("/login", {
          method: "POST",
          body: { name: userName, password }
        });
        const nextApiKey = login.api_key;
        const nextRole: UserRole = login.user_id === 0 ? "admin" : "user";
        setApiKey(nextApiKey);
        setRole(nextRole);
        localStorage.setItem(API_KEY_STORAGE_KEY, nextApiKey);
        localStorage.setItem(ROLE_STORAGE_KEY, nextRole);
        const fallback = defaultModule(nextRole);
        setActiveModule(fallback);
        setHashRoute(nextRole, fallback);
        notify("success", t("app.loginAs", { role: nextRole }));
      } finally {
        setLoginLoading(false);
      }
    },
    [notify, t]
  );

  const onLogout = useCallback(() => {
    localStorage.removeItem(API_KEY_STORAGE_KEY);
    localStorage.removeItem(ROLE_STORAGE_KEY);
    setApiKey(null);
    setRole(null);
    setActiveModule("");
    if (window.location.hash) {
      window.location.hash = "";
    }
    notify("info", t("app.loggedOut"));
  }, [notify, t]);

  const content = useMemo(() => {
    if (!apiKey || !role) {
      return null;
    }
    if (role === "admin") {
      switch (activeModule as AdminNavId) {
        case "global-settings":
          return <GlobalSettingsModule apiKey={apiKey} notify={notify} />;
        case "providers":
          return <ProvidersModule apiKey={apiKey} notify={notify} />;
        case "users":
          return <UsersModule apiKey={apiKey} notify={notify} />;
        case "requests":
          return <RequestsModule apiKey={apiKey} notify={notify} />;
        case "usage":
          return <UsageModule apiKey={apiKey} notify={notify} />;
        case "about":
          return <AboutModule />;
        default:
          return null;
      }
    }

    switch (activeModule as UserNavId) {
      case "my-keys":
        return <MyKeysModule apiKey={apiKey} notify={notify} />;
      case "my-usage":
        return <MyUsageModule apiKey={apiKey} notify={notify} />;
      case "about":
        return <AboutModule />;
      default:
        return null;
    }
  }, [activeModule, apiKey, notify, role]);

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

  const toggleTheme = () => {
    setThemeMode((prev) => (prev === "dark" ? "light" : "dark"));
  };
  const isDarkTheme = themeMode === "dark";

  return (
    <div className="app-shell">
      <header className="topbar-shell">
        <div className="topbar-panel mx-auto flex w-full max-w-[1700px] flex-col gap-3 px-4 py-3 md:flex-row md:items-center md:justify-between md:gap-4">
          <div className="flex min-w-0 flex-wrap items-center gap-2 md:gap-3">
            <h1 className="topbar-title">{t("app.title")}</h1>
            <Badge active>{role}</Badge>
            <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[11px] text-muted">
              v{appVersion}
            </code>
            <code className="rounded border border-border px-1.5 py-0.5 font-mono text-[11px] text-muted">
              {appCommit}
            </code>
          </div>
          <div className="flex w-full items-center justify-between gap-2 md:w-auto md:justify-end md:gap-3">
            <div className="flex items-center gap-2">
              <div className="topbar-segmented" role="group" aria-label={t("app.locale.switcher")}>
                <button
                  type="button"
                  className={`topbar-segmented-item ${locale === "zh" ? "topbar-segmented-item-active" : ""}`}
                  onClick={() => setLocale("zh")}
                >
                  CN
                </button>
                <button
                  type="button"
                  className={`topbar-segmented-item ${locale === "en" ? "topbar-segmented-item-active" : ""}`}
                  onClick={() => setLocale("en")}
                >
                  EN
                </button>
              </div>
              <button
                type="button"
                className="topbar-theme-toggle"
                onClick={toggleTheme}
                aria-label={t("app.theme.toggle")}
                title={t("app.theme.toggle")}
              >
                {isDarkTheme ? (
                  <svg viewBox="0 0 24 24" className="topbar-theme-icon" aria-hidden="true">
                    <path
                      fill="currentColor"
                      d="M21.64 13a1 1 0 0 0-1.06-.57 8 8 0 0 1-9-9 1 1 0 0 0-1.63-.93A10 10 0 1 0 22.5 14.67a1 1 0 0 0-.86-1.67z"
                    />
                  </svg>
                ) : (
                  <svg viewBox="0 0 24 24" className="topbar-theme-icon" aria-hidden="true">
                    <path
                      fill="currentColor"
                      d="M12 4a1 1 0 0 1 1 1v1.35a1 1 0 1 1-2 0V5a1 1 0 0 1 1-1zm0 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm8-5a1 1 0 1 1 0 2h-1.35a1 1 0 1 1 0-2zM6.35 12a1 1 0 1 1 0 2H5a1 1 0 1 1 0-2zm10.27 5.66a1 1 0 0 1 1.42 0l.95.96a1 1 0 1 1-1.41 1.41l-.96-.95a1 1 0 0 1 0-1.42zM5.04 6.45a1 1 0 0 1 1.41 0l.96.95A1 1 0 0 1 6 8.82l-.95-.96a1 1 0 0 1 0-1.41zm13.95 0a1 1 0 0 1 0 1.41l-.95.96a1 1 0 1 1-1.42-1.42l.96-.95a1 1 0 0 1 1.41 0zM7.41 16.59a1 1 0 0 1 0 1.42l-.96.95a1 1 0 0 1-1.41-1.41l.95-.96a1 1 0 0 1 1.42 0z"
                    />
                  </svg>
                )}
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
          onChange={(moduleId) => {
            setActiveModule(moduleId);
            setHashRoute(role, moduleId);
          }}
        />
        <section className="content-shell">{content}</section>
      </main>
      <Toast toast={toast} />
    </div>
  );
}
