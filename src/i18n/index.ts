import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const LOCALE_STORAGE_KEY = "vibe-locale";

export const SUPPORTED_LOCALES = ["en", "zh-CN"] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

function normalizeLocale(value: string | null | undefined): Locale {
  if (!value) return "en";
  if (value === "zh" || value.startsWith("zh-")) return "zh-CN";
  if (SUPPORTED_LOCALES.includes(value as Locale)) return value as Locale;
  return "en";
}

function detectInitialLocale(): Locale {
  const stored = readStoredLocale();
  if (stored) return normalizeLocale(stored);
  return normalizeLocale(readNavigatorLanguage());
}

function readStoredLocale(): string | null {
  if (typeof localStorage === "undefined") return null;
  try {
    return localStorage.getItem(LOCALE_STORAGE_KEY);
  } catch {
    return null;
  }
}

function readNavigatorLanguage(): string | null {
  return typeof navigator === "undefined" ? null : navigator.language;
}

function persistLocale(locale: string) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  } catch {
    // Ignore storage failures; language switching should still work in-memory.
  }
}

function syncDocumentLanguage(locale: string) {
  if (typeof document === "undefined") return;
  document.documentElement.lang = locale;
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    "zh-CN": { translation: zhCN },
  },
  lng: detectInitialLocale(),
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

i18n.on("languageChanged", (lng) => {
  syncDocumentLanguage(lng);
  persistLocale(lng);
});

syncDocumentLanguage(i18n.language);

export function setLocale(locale: Locale) {
  void i18n.changeLanguage(locale);
}

export default i18n;
