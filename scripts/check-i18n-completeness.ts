/**
 * i18n completeness check: deep-compares leaf key paths between the English
 * (reference) and all other locale bundles.
 *
 * - zh-CN is a fully-supported locale: mismatches FAIL (exit 1).
 * - All other non-en locales (ja, es, ko, ru, zh-TW) are beta: mismatches
 *   produce WARNINGS but do not fail.
 *
 * Usage: pnpm check:i18n
 */
import { readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const localesDir = resolve(__dirname, "../src/i18n/locales");

/** Locales that must be perfectly in sync with en (mismatch = CI failure). */
const STRICT_LOCALES = new Set(["zh-CN"]);

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function flattenKeys(obj: unknown, prefix = ""): string[] {
  if (!isPlainObject(obj)) return [prefix];
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return flattenKeys(value, path);
  });
}

async function loadLocale(fileName: string): Promise<unknown> {
  const module = await import(pathToFileURL(join(localesDir, fileName)).href);
  return module.default;
}

async function main(): Promise<void> {
  const files = readdirSync(localesDir).filter((f) => f.endsWith(".ts"));
  if (files.length === 0) {
    console.error(`No locale files found in ${localesDir}`);
    process.exit(1);
  }

  const enFile = files.find((f) => f === "en.ts");
  if (!enFile) {
    console.error(`Reference locale 'en.ts' not found in ${localesDir}`);
    process.exit(1);
  }

  const enKeys = new Set(flattenKeys(await loadLocale(enFile)));

  let hasFailures = false;
  let hasWarnings = false;

  for (const file of files.sort()) {
    if (file === "en.ts") continue;

    const localeName = file.replace(/\.ts$/, "");
    const localeKeys = new Set(flattenKeys(await loadLocale(file)));

    const missing = [...enKeys].filter((k) => !localeKeys.has(k)).sort();
    const extra = [...localeKeys].filter((k) => !enKeys.has(k)).sort();

    if (missing.length === 0 && extra.length === 0) {
      console.log(`i18n check passed for ${localeName}.`);
      continue;
    }

    const isStrict = STRICT_LOCALES.has(localeName);
    const level = isStrict ? "FAIL" : "WARN";
    console.error(`[${level}] i18n completeness check for ${localeName}:`);
    if (missing.length > 0) {
      console.error(`  Missing (${missing.length}):`);
      for (const key of missing) {
        console.error(`    - ${key}`);
      }
    }
    if (extra.length > 0) {
      console.error(`  Extra (${extra.length}):`);
      for (const key of extra) {
        console.error(`    - ${key}`);
      }
    }

    if (isStrict) {
      hasFailures = true;
    } else {
      hasWarnings = true;
    }
  }

  if (hasFailures) {
    console.error("\ni18n completeness check failed (strict locales had mismatches).");
    process.exit(1);
  }

  if (hasWarnings) {
    console.log("\ni18n completeness check passed with warnings (beta locales had mismatches).");
  } else {
    console.log("\ni18n completeness check passed.");
  }
}

await main();
