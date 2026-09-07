/**
 * i18n completeness check: compares leaf key paths AND values between the
 * English reference bundle and all other locale files.
 *
 * FUN-21: key-only checks cannot catch copy-pasted English. Non-en values that
 * still match English are failures when they are `errors.*` (except allowlisted
 * technical tokens) or contain 3+ visible English words.
 *
 * Usage: pnpm check:i18n
 */
import { readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEFAULT_LOCALES_DIR = resolve(__dirname, "../src/i18n/locales");

export const STRICT_LOCALES = ["zh-CN", "zh-TW", "ja", "ko", "ru", "es"] as const;

/** Keys that are intentionally identical across locales (product/protocol tokens). */
export const VALUE_ALLOWLIST_KEYS = new Set([
  "app.name",
  "trayMenu.title",
  "about.authorValue",
  "about.licenseValue",
  "errors.report.url",
  "newDownload.fileKindDash",
  "newDownload.sha256",
  "settings.ffmpegPath.placeholder",
  "settings.proxyUrlPlaceholder",
  "settings.proxyNoProxyPlaceholder",
  "settings.ruleHostPatternPlaceholder",
  "settings.environmentItemFfmpeg",
  "settings.externalToolsSummary",
  "task.fileType.torrent",
  "taskDetails.requestIfRange",
]);

export const VALUE_ALLOWLIST_PREFIXES = ["locale.", "format.byteUnit.", "taskList.failure_"] as const;

export function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function flattenLeaves(obj: unknown, prefix = ""): Record<string, string> {
  if (typeof obj === "string") {
    return prefix ? { [prefix]: obj } : {};
  }
  if (!isPlainObject(obj)) return {};
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key;
    Object.assign(out, flattenLeaves(value, path));
  }
  return out;
}

export function placeholderNames(value: string): string[] {
  return [...value.matchAll(/\{\{\s*([a-zA-Z0-9_]+)\s*\}\}/g)].map((m) => m[1]).sort();
}

/** Latin words that remain after stripping interpolation placeholders. */
export function visibleEnglishWords(value: string): string[] {
  const visible = value.replace(/\{\{[^}]+\}\}/g, " ");
  return visible.match(/\b[A-Za-z]{2,}\b/g) ?? [];
}

export function isValueAllowlisted(key: string): boolean {
  if (VALUE_ALLOWLIST_KEYS.has(key)) return true;
  return VALUE_ALLOWLIST_PREFIXES.some((prefix) => key.startsWith(prefix));
}

export type UntranslatedHit = {
  key: string;
  reason: "errors" | "english-sentence";
  words: number;
};

/**
 * A non-en leaf is untranslated when it still equals English and is not an
 * allowlisted token. `errors.*` always fail (users must see localized failures).
 * Other namespaces fail only when 3+ visible English words remain, matching FUN-21.
 */
export function findUntranslatedLeaves(
  enLeaves: Record<string, string>,
  localeLeaves: Record<string, string>,
): UntranslatedHit[] {
  const hits: UntranslatedHit[] = [];
  for (const [key, enValue] of Object.entries(enLeaves)) {
    const localeValue = localeLeaves[key];
    if (localeValue === undefined || localeValue !== enValue) continue;
    if (isValueAllowlisted(key)) continue;
    const words = visibleEnglishWords(enValue).length;
    if (key.startsWith("errors.")) {
      hits.push({ key, reason: "errors", words });
      continue;
    }
    if (words >= 3) {
      hits.push({ key, reason: "english-sentence", words });
    }
  }
  return hits.sort((a, b) => a.key.localeCompare(b.key));
}

export type PlaceholderHit = {
  key: string;
  en: string[];
  locale: string[];
};

export function findPlaceholderMismatches(
  enLeaves: Record<string, string>,
  localeLeaves: Record<string, string>,
): PlaceholderHit[] {
  const hits: PlaceholderHit[] = [];
  for (const [key, enValue] of Object.entries(enLeaves)) {
    const localeValue = localeLeaves[key];
    if (localeValue === undefined) continue;
    const en = placeholderNames(enValue);
    const locale = placeholderNames(localeValue);
    if (en.join("\0") !== locale.join("\0")) {
      hits.push({ key, en, locale });
    }
  }
  return hits.sort((a, b) => a.key.localeCompare(b.key));
}

export type LocaleCheckResult = {
  locale: string;
  missing: string[];
  extra: string[];
  untranslated: UntranslatedHit[];
  placeholders: PlaceholderHit[];
};

export function checkLocale(
  locale: string,
  enLeaves: Record<string, string>,
  localeLeaves: Record<string, string>,
): LocaleCheckResult {
  const enKeys = Object.keys(enLeaves);
  const localeKeys = Object.keys(localeLeaves);
  return {
    locale,
    missing: enKeys.filter((k) => !(k in localeLeaves)).sort(),
    extra: localeKeys.filter((k) => !(k in enLeaves)).sort(),
    untranslated: findUntranslatedLeaves(enLeaves, localeLeaves),
    placeholders: findPlaceholderMismatches(enLeaves, localeLeaves),
  };
}

export function localeHasFailures(result: LocaleCheckResult): boolean {
  return (
    result.missing.length > 0 ||
    result.extra.length > 0 ||
    result.untranslated.length > 0 ||
    result.placeholders.length > 0
  );
}

async function loadLocale(localesDir: string, fileName: string): Promise<unknown> {
  const module = await import(pathToFileURL(join(localesDir, fileName)).href);
  return module.default;
}

export async function runCompletenessCheck(localesDir = DEFAULT_LOCALES_DIR): Promise<number> {
  const files = readdirSync(localesDir).filter((f) => f.endsWith(".ts"));
  if (files.length === 0) {
    console.error(`No locale files found in ${localesDir}`);
    return 1;
  }

  const enFile = files.find((f) => f === "en.ts");
  if (!enFile) {
    console.error(`Reference locale 'en.ts' not found in ${localesDir}`);
    return 1;
  }

  const enLeaves = flattenLeaves(await loadLocale(localesDir, enFile));
  let hasFailures = false;

  for (const file of files.sort()) {
    if (file === "en.ts") continue;
    const localeName = file.replace(/\.ts$/, "");
    const localeLeaves = flattenLeaves(await loadLocale(localesDir, file));
    const result = checkLocale(localeName, enLeaves, localeLeaves);
    const failed = localeHasFailures(result);
    const isStrict = (STRICT_LOCALES as readonly string[]).includes(localeName);

    if (!failed) {
      console.log(`i18n check passed for ${localeName} (${Object.keys(localeLeaves).length} keys).`);
      continue;
    }

    const level = isStrict ? "FAIL" : "WARN";
    console.error(`[${level}] i18n completeness check for ${localeName}:`);
    if (result.missing.length > 0) {
      console.error(`  Missing keys (${result.missing.length}):`);
      for (const key of result.missing) console.error(`    - ${key}`);
    }
    if (result.extra.length > 0) {
      console.error(`  Extra keys (${result.extra.length}):`);
      for (const key of result.extra) console.error(`    - ${key}`);
    }
    if (result.placeholders.length > 0) {
      console.error(`  Placeholder mismatches (${result.placeholders.length}):`);
      for (const hit of result.placeholders) {
        console.error(`    - ${hit.key}: en={${hit.en.join(",")}} locale={${hit.locale.join(",")}}`);
      }
    }
    if (result.untranslated.length > 0) {
      const errors = result.untranslated.filter((h) => h.reason === "errors");
      const sentences = result.untranslated.filter((h) => h.reason === "english-sentence");
      if (errors.length > 0) {
        console.error(`  Untranslated errors.* (${errors.length}):`);
        for (const hit of errors) console.error(`    - ${hit.key}`);
      }
      if (sentences.length > 0) {
        console.error(`  Untranslated English copy (${sentences.length}):`);
        for (const hit of sentences) console.error(`    - ${hit.key}`);
      }
    }

    if (isStrict) hasFailures = true;
  }

  if (hasFailures) {
    console.error("\ni18n completeness check failed (key, placeholder, or value mismatches).");
    return 1;
  }

  console.log("\ni18n completeness check passed.");
  return 0;
}

const invoked = process.argv[1] ? resolve(process.argv[1]) : "";
const thisFile = resolve(fileURLToPath(import.meta.url));
if (invoked === thisFile) {
  process.exit(await runCompletenessCheck());
}
