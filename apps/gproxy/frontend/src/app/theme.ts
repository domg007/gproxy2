import type { ThemeMode } from "../lib/types";

export const THEME_KEY = "gproxy_theme";

export function readStoredTheme(): ThemeMode {
  const raw = localStorage.getItem(THEME_KEY);
  if (raw === "light" || raw === "dark" || raw === "system") {
    return raw;
  }
  return "system";
}

export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  const resolved = resolveTheme(mode);
  root.dataset.theme = resolved;
}

export function persistTheme(mode: ThemeMode): void {
  localStorage.setItem(THEME_KEY, mode);
}

function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return mode;
}
