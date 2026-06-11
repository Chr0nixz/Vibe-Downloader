import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";
import zhTW from "./locales/zh-TW";
import ja from "./locales/ja";
import ko from "./locales/ko";
import ru from "./locales/ru";
import es from "./locales/es";

export const LOCALE_STORAGE_KEY = "vibe-locale";

export const SUPPORTED_LOCALES = ["en", "zh-CN", "zh-TW", "ja", "ko", "ru", "es"] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

function normalizeLocale(value: string | null | undefined): Locale {
  if (!value) return "en";
  if (
    value === "zh-TW" ||
    value === "zh-Hant" ||
    value === "zh-HK" ||
    value === "zh-MO" ||
    /^zh-(TW|HK|MO|Hant)/i.test(value)
  )
    return "zh-TW";
  if (value === "zh" || value.startsWith("zh-")) return "zh-CN";
  if (value === "ja" || value.startsWith("ja-")) return "ja";
  if (value === "ko" || value.startsWith("ko-")) return "ko";
  if (value === "ru" || value.startsWith("ru-")) return "ru";
  if (value === "es" || value.startsWith("es-")) return "es";
  if (SUPPORTED_LOCALES.includes(value as Locale)) return value as Locale;
  return "en";
}

function detectInitialLocale(): Locale {
  const stored = localStorage.getItem(LOCALE_STORAGE_KEY);
  if (stored) return normalizeLocale(stored);
  return normalizeLocale(navigator.language);
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    "zh-CN": { translation: zhCN },
    "zh-TW": { translation: zhTW },
    ja: { translation: ja },
    ko: { translation: ko },
    ru: { translation: ru },
    es: { translation: es },
  },
  lng: detectInitialLocale(),
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
  localStorage.setItem(LOCALE_STORAGE_KEY, lng);
});

document.documentElement.lang = i18n.language;

export function setLocale(locale: Locale) {
  void i18n.changeLanguage(locale);
}

export default i18n;
