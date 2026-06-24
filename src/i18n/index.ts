import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const LOCALE_STORAGE_KEY = "vibe-locale";

/**
 * Single source of truth for locale metadata. All locale constants below are
 * derived from this registry to prevent drift between SUPPORTED_LOCALES,
 * STABLE_LOCALES, and LOCALE_LABEL_KEYS.
 */
const LOCALE_REGISTRY = [
  { code: "en", labelKey: "locale.en", stable: true },
  { code: "zh-CN", labelKey: "locale.zhCN", stable: true },
  { code: "zh-TW", labelKey: "locale.zhTW", stable: false },
  { code: "ja", labelKey: "locale.ja", stable: false },
  { code: "ko", labelKey: "locale.ko", stable: false },
  { code: "ru", labelKey: "locale.ru", stable: false },
  { code: "es", labelKey: "locale.es", stable: false },
] as const;

export type Locale = (typeof LOCALE_REGISTRY)[number]["code"];

export const SUPPORTED_LOCALES: readonly Locale[] = LOCALE_REGISTRY.map((e) => e.code);

/** Locales with complete translation coverage (~670 keys). Exposed in the language selector. */
export const STABLE_LOCALES: readonly Locale[] = LOCALE_REGISTRY.filter((e) => e.stable).map((e) => e.code);

/** Maps locale code → i18n key for the locale's display name. */
export const LOCALE_LABEL_KEYS: Record<string, string> = Object.fromEntries(
  LOCALE_REGISTRY.map((e) => [e.code, e.labelKey]),
);

/**
 * Eagerly bundled locales (first-screen). All other locales are lazy-loaded
 * via dynamic import() to avoid shipping ~130 KB of incomplete translations
 * in the initial bundle.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type LocaleBundle = Record<string, any>;

const EAGER_RESOURCES: Record<string, LocaleBundle> = {
  en,
  "zh-CN": zhCN,
};

const loadedLocales = new Set<string>(Object.keys(EAGER_RESOURCES));

const LAZY_LOADERS: Record<string, () => Promise<{ default: LocaleBundle }>> = {
  "zh-TW": () => import("./locales/zh-TW" /* webpackChunkName: "locale-zh-TW" */),
  ja: () => import("./locales/ja" /* webpackChunkName: "locale-ja" */),
  ko: () => import("./locales/ko" /* webpackChunkName: "locale-ko" */),
  ru: () => import("./locales/ru" /* webpackChunkName: "locale-ru" */),
  es: () => import("./locales/es" /* webpackChunkName: "locale-es" */),
};

async function loadLocaleBundle(locale: string): Promise<LocaleBundle | undefined> {
  if (loadedLocales.has(locale)) return undefined;
  const loader = LAZY_LOADERS[locale];
  if (!loader) return undefined;
  const mod = await loader();
  loadedLocales.add(locale);
  return mod.default;
}

function normalizeLocale(value: string | null | undefined): Locale {
  if (!value) return "en";
  if (value === "zh-TW" || value === "zh-HK" || value === "zh-MO") return "zh-TW";
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

function buildInitResources(): Record<string, { translation: Record<string, string> }> {
  const resources: Record<string, { translation: Record<string, string> }> = {};
  for (const [locale, bundle] of Object.entries(EAGER_RESOURCES)) {
    resources[locale] = { translation: bundle };
  }
  return resources;
}

const initialLocale = detectInitialLocale();

void i18n.use(initReactI18next).init({
  resources: buildInitResources(),
  lng: initialLocale,
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

// If the initial locale is not eagerly bundled, load it in the background.
// Users will briefly see English fallback, then switch once loaded.
if (!loadedLocales.has(initialLocale)) {
  void loadLocaleBundle(initialLocale).then((bundle) => {
    if (bundle) {
      i18n.addResourceBundle(initialLocale, "translation", bundle, true, true);
      void i18n.changeLanguage(initialLocale);
    }
  });
}

i18n.on("languageChanged", (lng) => {
  syncDocumentLanguage(lng);
  persistLocale(lng);
});

syncDocumentLanguage(i18n.language);

export async function setLocale(locale: Locale) {
  const bundle = await loadLocaleBundle(locale);
  if (bundle) {
    i18n.addResourceBundle(locale, "translation", bundle, true, true);
  }
  void i18n.changeLanguage(locale);
}

export default i18n;
