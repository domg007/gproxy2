import { Activity, Gauge, KeyRound, UserCog } from "lucide-react";
import type { NavItem } from "./nav";

export const PORTAL_NAV: NavItem[] = [
  { to: "/account/keys",     icon: KeyRound, labelKey: "nav.keys" },
  { to: "/account/usage",    icon: Activity, labelKey: "nav.usage" },
  { to: "/account/limits",   icon: Gauge,    labelKey: "nav.limits" },
  { to: "/account/security", icon: UserCog,  labelKey: "nav.account" },
];
