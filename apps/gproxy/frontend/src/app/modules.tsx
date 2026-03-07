import { GlobalSettingsModule } from "../modules/admin/GlobalSettingsModule";
import { ProvidersModule } from "../modules/admin/ProvidersModule";
import { RequestsModule } from "../modules/admin/RequestsModule";
import { UsageModule } from "../modules/admin/UsageModule";
import { UsersModule } from "../modules/admin/UsersModule";
import { AboutModule } from "../modules/shared/AboutModule";
import { MyKeysModule } from "../modules/user/MyKeysModule";
import { MyUsageModule } from "../modules/user/MyUsageModule";
import type { NavItem } from "../components/Nav";
import type { UserRole } from "../lib/types";

export const USER_NAV_IDS = ["my-keys", "my-usage", "about"] as const;

export type AdminNavId =
  | "global-settings"
  | "providers"
  | "users"
  | "requests"
  | "usage"
  | "about";

export type UserNavId = (typeof USER_NAV_IDS)[number];

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export function defaultModule(role: UserRole): string {
  return role === "admin" ? "providers" : USER_NAV_IDS[0];
}

export function buildAdminNavItems(t: TranslateFn): NavItem[] {
  return [
    { id: "global-settings", label: t("app.nav.globalSettings") },
    { id: "providers", label: t("app.nav.providers") },
    { id: "users", label: t("app.nav.users") },
    { id: "requests", label: t("app.nav.requests") },
    { id: "usage", label: t("app.nav.usage") },
    { id: "about", label: t("app.nav.about") }
  ];
}

export function buildUserNavItems(t: TranslateFn): NavItem[] {
  return [
    { id: "my-keys", label: t("app.nav.myKeys") },
    { id: "my-usage", label: t("app.nav.myUsage") },
    { id: "about", label: t("app.nav.about") }
  ];
}

export function renderActiveModule({
  role,
  activeModule,
  apiKey,
  notify
}: {
  role: UserRole | null;
  activeModule: string;
  apiKey: string | null;
  notify: NotifyFn;
}) {
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
}
