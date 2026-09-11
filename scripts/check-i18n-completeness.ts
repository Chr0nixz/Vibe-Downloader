/**
 * i18n completeness check.
 *
 * 1. Compares leaf key paths AND values between the English reference bundle and
 *    all other locale files. FUN-21: key-only checks cannot catch copy-pasted
 *    English. Non-en values that still match English are failures when they are
 *    `errors.*` (except allowlisted technical tokens) or contain 3+ visible
 *    English words.
 * 2. Resolves plural forms against each locale's CLDR categories rather than
 *    demanding a byte-identical key set (FUN-22).
 * 3. Verifies every literal `t("...")` key in `src/**` exists in the English
 *    bundle, which the locale-to-locale diff cannot see.
 *
 * Usage: pnpm check:i18n
 */
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
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

/**
 * FUN-22: i18next resolves `{{count}}` keys to a CLDR plural category suffix
 * (`key_one`, `key_few`, `key_many`, …) and falls back to the bare key when the
 * variant is absent. A strict key-set equality would therefore reject the
 * correct per-language fix (ru needs `_few`/`_many`, zh/ja/ko need neither), so
 * plural variants are checked against the locale's own CLDR categories.
 */
export const PLURAL_SUFFIXES = ["zero", "one", "two", "few", "many", "other"] as const;

const PLURAL_SUFFIX_RE = /^(.*)_(zero|one|two|few|many|other)$/;

/** CLDR plural categories the locale actually selects, e.g. ru → one/few/many/other. */
export function pluralCategories(locale: string): Set<string> {
  try {
    return new Set(new Intl.PluralRules(locale).resolvedOptions().pluralCategories);
  } catch {
    return new Set(["other"]);
  }
}

/**
 * Bases the English bundle declares as pluralised: `base` exists AND has at
 * least one `<base>_<category>` sibling AND interpolates `{{count}}`. Anchoring
 * on `{{count}}` keeps look-alikes such as `taskList.failure_other` (a plain
 * value, not a plural form of `taskList.failure`) out of the plural model.
 */
export function findPluralBases(enLeaves: Record<string, string>): Set<string> {
  const bases = new Set<string>();
  for (const key of Object.keys(enLeaves)) {
    const match = PLURAL_SUFFIX_RE.exec(key);
    if (!match) continue;
    const base = match[1];
    if ((enLeaves[base] ?? "").includes("{{count}}")) bases.add(base);
  }
  return bases;
}

function pluralSuffixOf(key: string): string | null {
  const match = PLURAL_SUFFIX_RE.exec(key);
  return match ? match[2] : null;
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
  unreachablePlurals: string[];
  untranslated: UntranslatedHit[];
  placeholders: PlaceholderHit[];
};

export function checkLocale(
  locale: string,
  enLeaves: Record<string, string>,
  localeLeaves: Record<string, string>,
): LocaleCheckResult {
  const pluralBases = findPluralBases(enLeaves);
  const categories = pluralCategories(locale);

  /** `<base>_<category>` where en declares `<base>` as pluralised. */
  const pluralCategoryOf = (key: string): string | null => {
    const suffix = pluralSuffixOf(key);
    if (!suffix) return null;
    return pluralBases.has(key.slice(0, key.length - suffix.length - 1)) ? suffix : null;
  };

  // A plural variant is required only where the locale actually selects that
  // category; elsewhere i18next falls back to the bare key.
  const missing = Object.keys(enLeaves)
    .filter((key) => {
      if (key in localeLeaves) return false;
      const category = pluralCategoryOf(key);
      return category === null || categories.has(category);
    })
    .sort();

  const extra: string[] = [];
  const unreachablePlurals: string[] = [];
  for (const key of Object.keys(localeLeaves)) {
    const category = pluralCategoryOf(key);
    if (category !== null && !categories.has(category)) {
      unreachablePlurals.push(key);
      continue;
    }
    if (!(key in enLeaves) && category === null) extra.push(key);
  }

  return {
    locale,
    missing,
    extra: extra.sort(),
    unreachablePlurals: unreachablePlurals.sort(),
    untranslated: findUntranslatedLeaves(enLeaves, localeLeaves),
    placeholders: findPlaceholderMismatches(enLeaves, localeLeaves),
  };
}

export function localeHasFailures(result: LocaleCheckResult): boolean {
  return (
    result.missing.length > 0 ||
    result.extra.length > 0 ||
    result.unreachablePlurals.length > 0 ||
    result.untranslated.length > 0 ||
    result.placeholders.length > 0
  );
}

async function loadLocale(localesDir: string, fileName: string): Promise<unknown> {
  const module = await import(pathToFileURL(join(localesDir, fileName)).href);
  return module.default;
}

/**
 * Nothing else verifies that a `t("...")` literal actually exists in the English
 * bundle — the locale-to-locale diff above cannot see a key that is absent from
 * every bundle — so a typo would ship and render the raw key to users. Only
 * fully literal keys are scanned: template literals and concatenations are
 * resolved at runtime and cannot be checked statically.
 */
export const SOURCE_KEY_RE = /(?:^|[^\w.$])(?:i18n\.)?t\(\s*["']([^"'\n]+)["']\s*[,)]/g;

export function extractLiteralTranslationKeys(source: string): string[] {
  const keys: string[] = [];
  for (const match of source.matchAll(SOURCE_KEY_RE)) keys.push(match[1]);
  return keys;
}

const SOURCE_ROOT = resolve(__dirname, "../src");
/** Locale bundles are translation content; other files are call sites. */
const SOURCE_SKIP_DIRS = new Set(["locales"]);

function collectSourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (SOURCE_SKIP_DIRS.has(entry.name)) continue;
      files.push(...collectSourceFiles(path));
      continue;
    }
    if (!/\.(ts|tsx)$/.test(entry.name)) continue;
    if (/\.(test|spec)\.(ts|tsx)$/.test(entry.name)) continue;
    files.push(path);
  }
  return files;
}

export type SourceKeyHit = { key: string; file: string; line: number };

/** Literal `t()` keys referenced in `src/**` that the English bundle does not define. */
export function findMissingSourceKeys(enLeaves: Record<string, string>, sourceRoot = SOURCE_ROOT): SourceKeyHit[] {
  const repoRoot = resolve(__dirname, "..");
  const hits: SourceKeyHit[] = [];
  for (const file of collectSourceFiles(sourceRoot)) {
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(SOURCE_KEY_RE)) {
      const key = match[1];
      if (key in enLeaves) continue;
      hits.push({
        key,
        file: relative(repoRoot, file).replace(/\\/g, "/"),
        line: source.slice(0, match.index).split("\n").length,
      });
    }
  }
  return hits.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
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
    if (result.unreachablePlurals.length > 0) {
      console.error(`  Unreachable plural forms for ${localeName} (${result.unreachablePlurals.length}):`);
      console.error(
        `    i18next never selects these categories for this locale; delete or translate the base key instead.`,
      );
      for (const key of result.unreachablePlurals) console.error(`    - ${key}`);
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

  const missingSourceKeys = findMissingSourceKeys(enLeaves);
  if (missingSourceKeys.length > 0) {
    console.error(`\n[FAIL] ${missingSourceKeys.length} literal t() key(s) are not defined in en.ts:`);
    for (const hit of missingSourceKeys) console.error(`    - ${hit.file}:${hit.line} → ${hit.key}`);
    hasFailures = true;
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
