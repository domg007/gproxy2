import { Link } from "@tanstack/react-router";
import { LayoutDashboard, Plug, Route as RouteIcon, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

interface NavItem {
  to: string;
  icon: LucideIcon;
  labelKey: string;
}

export const NAV_ITEMS: NavItem[] = [
  { to: "/", icon: LayoutDashboard, labelKey: "nav.dashboard" },
  { to: "/providers", icon: Plug, labelKey: "nav.providers" },
  { to: "/routes", icon: RouteIcon, labelKey: "nav.routes" },
];

export function NavList({ compact, onNavigate }: { compact?: boolean; onNavigate?: () => void }) {
  const { t } = useTranslation();
  return (
    <nav className="grid gap-1 px-2">
      {NAV_ITEMS.map((item) => (
        <Link
          key={item.to}
          to={item.to}
          onClick={onNavigate}
          activeOptions={{ exact: item.to === "/" }}
          activeProps={{ "data-active": "true" as const }}
          className={cn(
            "flex items-center gap-3 rounded-md px-3 py-2 text-sm text-muted-foreground",
            "hover:bg-accent hover:text-accent-foreground",
            "data-[active=true]:bg-accent data-[active=true]:font-medium data-[active=true]:text-accent-foreground",
            compact && "justify-center px-2",
          )}
          title={compact ? t(item.labelKey) : undefined}
        >
          <item.icon className="size-4 shrink-0" />
          <span className={cn(compact && "hidden xl:inline")}>{t(item.labelKey)}</span>
        </Link>
      ))}
    </nav>
  );
}
