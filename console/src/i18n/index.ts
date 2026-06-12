import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import resourcesToBackend from "i18next-resources-to-backend";

export const SUPPORTED_LANGS = [
  { code: "en", label: "English" },
  { code: "zh-CN", label: "中文" },
] as const;
export type LangCode = (typeof SUPPORTED_LANGS)[number]["code"];

const STORAGE_KEY = "gproxy-console-lang";

void i18n
  .use(
    resourcesToBackend(
      (lng: string, ns: string) => import(`../locales/${lng}/${ns}.json`),
    ),
  )
  .use(initReactI18next)
  .init({
    lng: localStorage.getItem(STORAGE_KEY) ?? "en",
    fallbackLng: "en",
    ns: ["common"],
    defaultNS: "common",
    interpolation: { escapeValue: false },
  });

export function setLanguage(code: LangCode) {
  localStorage.setItem(STORAGE_KEY, code);
  void i18n.changeLanguage(code);
}

export default i18n;
