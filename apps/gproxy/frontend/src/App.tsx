import { useCallback, useEffect, useMemo, useState } from "react";

import { LoginGate } from "./components/LoginGate";
import { Sidebar, type NavItem } from "./components/Sidebar";
import { Toast } from "./components/Toast";
import { Badge, Button } from "./components/ui";
import { useI18n } from "./i18n";
import { request, formatApiError } from "./lib/api";
import { mask } from "./lib/format";
import type { ProviderDetail, ProviderSummary, ToastState } from "./lib/types";
import { AboutSection } from "./sections/AboutSection";
import { OverviewSection } from "./sections/OverviewSection";
import { ProvidersSection } from "./sections/ProvidersSection";
import { UsageSection } from "./sections/UsageSection";
import { UsersSection } from "./sections/UsersSection";
import { LogQuerySection } from "./sections/LogQuerySection";

const KEY_STORAGE = "gproxy_admin_key";

type RouteId = "overview" | "providers" | "users" | "usage" | "events" | "about";
const DEFAULT_ROUTE: RouteId = "overview";

function readRouteFromHash(): RouteId {
  const raw = window.location.hash.replace(/^#/, "");
  if (raw === "credentials" || raw === "oauth") {
    return "providers";
  }
  const value = raw as RouteId;
  const allowed: RouteId[] = ["overview", "providers", "users", "usage", "events", "about"];
  return allowed.includes(value) ? value : DEFAULT_ROUTE;
}

export default function App() {
  const { t, language, setLanguage } = useI18n();
  const [adminKey, setAdminKey] = useState(() => localStorage.getItem(KEY_STORAGE) ?? "");
  const [authed, setAuthed] = useState(false);
  const [route, setRoute] = useState<RouteId>(() => readRouteFromHash());
  const [toast, setToast] = useState<ToastState>(null);
  const [providers, setProviders] = useState<ProviderDetail[]>([]);

  const notify = useCallback((kind: "success" | "error" | "info", message: string) => {
    setToast({ kind, message });
  }, []);

  useEffect(() => {
    if (!toast) {
      return;
    }
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    const onHash = () => setRoute(readRouteFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const goRoute = (next: string) => {
    const value = next as RouteId;
    window.location.hash = value;
    setRoute(value);
  };

  const validateLogin = useCallback(
    async (key: string) => {
      try {
        await request("/admin/health", { adminKey: key });
        localStorage.setItem(KEY_STORAGE, key);
        setAdminKey(key);
        setAuthed(true);
        return { ok: true };
      } catch (error) {
        return { ok: false, message: formatApiError(error) };
      }
    },
    []
  );

  const loadProviders = useCallback(async () => {
    if (!adminKey) {
      return;
    }
    try {
      const list = await request<{ providers: ProviderSummary[] }>("/admin/providers", {
        adminKey
      });
      const details = await Promise.all(
        (list.providers ?? []).map((provider) =>
          request<ProviderDetail>(`/admin/providers/${provider.name}`, {
            adminKey
          })
        )
      );
      setProviders(details);
    } catch (error) {
      notify("error", formatApiError(error));
    }
  }, [adminKey, notify]);

  useEffect(() => {
    if (!adminKey) {
      return;
    }
    void validateLogin(adminKey).then((result) => {
      if (!result.ok) {
        setAuthed(false);
      }
    });
  }, [adminKey, validateLogin]);

  useEffect(() => {
    if (!authed) {
      return;
    }
    void loadProviders();
  }, [authed, loadProviders]);

  const navItems: NavItem[] = useMemo(
    () => [
      { id: "overview", label: t("nav.overview") },
      { id: "providers", label: t("nav.providers") },
      { id: "users", label: t("nav.users") },
      { id: "usage", label: t("nav.usage") },
      { id: "events", label: t("nav.logs") },
      { id: "about", label: t("nav.about") }
    ],
    [t]
  );

  const logout = () => {
    localStorage.removeItem(KEY_STORAGE);
    setAdminKey("");
    setAuthed(false);
    setProviders([]);
  };

  const contentWidthClass = route === "events" ? "max-w-[1850px]" : "max-w-[1300px]";

  if (!authed) {
    return (
      <>
        <LoginGate initialKey={adminKey} onLogin={validateLogin} />
        <Toast toast={toast} />
      </>
    );
  }

  return (
    <div className="app-shell">
      <Toast toast={toast} />
      <div className={`mx-auto flex w-full ${contentWidthClass} flex-col gap-6 px-4 py-6 lg:flex-row lg:px-6 lg:py-8`}>
        <Sidebar active={route} onChange={goRoute} items={navItems} />
        <main className="flex-1 space-y-5">
          <header className="topbar-shell">
            <div>
              <h1 className="text-2xl font-semibold text-slate-900">{t("app.title")}</h1>
              <p className="mt-1 text-sm text-slate-500">{t("app.subtitle")}</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge>{t("app.key_mask")}: {mask(adminKey, 5, 4)}</Badge>
              <select
                className="select !w-auto"
                value={language}
                onChange={(event) => setLanguage(event.target.value as "en" | "zh_cn")}
                aria-label={t("app.language")}
              >
                <option value="zh_cn">简体中文</option>
                <option value="en">English</option>
              </select>
              <Button variant="neutral" onClick={logout}>{t("app.logout")}</Button>
            </div>
          </header>

          {route === "overview" ? (
            <OverviewSection
              adminKey={adminKey}
              onAdminKeyChange={(nextAdminKey) => {
                localStorage.setItem(KEY_STORAGE, nextAdminKey);
                setAdminKey(nextAdminKey);
              }}
              notify={notify}
            />
          ) : null}
          {route === "providers" ? <ProvidersSection adminKey={adminKey} notify={notify} /> : null}
          {route === "users" ? <UsersSection adminKey={adminKey} notify={notify} /> : null}
          {route === "usage" ? <UsageSection adminKey={adminKey} providers={providers} notify={notify} /> : null}
          {route === "events" ? <LogQuerySection adminKey={adminKey} notify={notify} /> : null}
          {route === "about" ? <AboutSection adminKey={adminKey} notify={notify} /> : null}
        </main>
      </div>
    </div>
  );
}
