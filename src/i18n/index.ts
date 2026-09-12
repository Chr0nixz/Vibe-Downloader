import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const LOCALE_STORAGE_KEY = "vibe-locale";

/** Every dotted leaf path in the English bundle: `"nav.all" | "task.status.ok" | …`. */
type TranslationLeaves<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends string ? `${Prefix}${K}` : TranslationLeaves<T[K], `${Prefix}${K}.`>;
}[keyof T & string];

/**
 * FUN-29: the type for *tables* that hold i18n keys as data — `labelKey` fields,
 * arrays of step keys, code-to-key maps. The i18next type augmentation in
 * `i18next.d.ts` covers a direct translate call with a literal string; without
 * this type, a table declared as `string` widens the key back to `string` and
 * the augmentation cannot help.
 */
export type TranslationKey = TranslationLeaves<typeof en>;

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
] as const satisfies readonly { code: string; labelKey: TranslationKey; stable: boolean }[];

export type Locale = (typeof LOCALE_REGISTRY)[number]["code"];

export const SUPPORTED_LOCALES: readonly Locale[] = LOCALE_REGISTRY.map((e) => e.code);

/** Locales with complete translation coverage (~670 keys). Exposed in the language selector. */
export const STABLE_LOCALES: readonly Locale[] = LOCALE_REGISTRY.filter((e) => e.stable).map((e) => e.code);

/** Maps locale code → i18n key for the locale's display name. */
export const LOCALE_LABEL_KEYS = Object.fromEntries(LOCALE_REGISTRY.map((e) => [e.code, e.labelKey])) as Record<
  Locale,
  TranslationKey
>;

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

/**
 * Chinese tags need script awareness before region: browsers emit `zh-Hant`,
 * `zh-Hant-TW` and `zh-Hant-HK`, and a generic `zh-*` branch would hand those
 * users a Simplified interface. The region-only tags (`zh-TW`/`zh-HK`/`zh-MO`)
 * stay listed because legacy values are still stored that way.
 */
const TRADITIONAL_CHINESE_RE = /^zh-(hant|tw|hk|mo)\b/;

function normalizeLocale(value: string | null | undefined): Locale {
  if (!value) return "en";
  const tag = value.toLowerCase();
  if (tag === "zh" || tag.startsWith("zh-")) {
    return TRADITIONAL_CHINESE_RE.test(tag) ? "zh-TW" : "zh-CN";
  }
  return SUPPORTED_LOCALES.find((locale) => locale.toLowerCase() === tag) ?? "en";
}

export function detectInitialLocale(): Locale {
  const stored = readStoredLocale();
  if (stored) {
    // UX-1: Explicit user choices (including beta languages selected in settings) are persisted via localStorage and trusted
    return normalizeLocale(stored);
  }
  // UX-1: Auto-detection only selects stable languages; beta requires explicit user choice
  const locale = normalizeLocale(readNavigatorLanguage());
  if (!STABLE_LOCALES.includes(locale)) {
    return "en";
  }
  return locale;
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
  // Await the switch so callers (and tests) can rely on i18n.language being
  // applied when this resolves; fire-and-forget callers are unaffected.
  await i18n.changeLanguage(locale);
}

export default i18n;
