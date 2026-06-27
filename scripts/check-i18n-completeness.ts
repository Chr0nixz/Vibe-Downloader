/**
 * i18n completeness check: deep-compares leaf key paths between the English
 * (reference) and Simplified Chinese (zh-CN) locale bundles. Exits with code 1
 * if any key is missing or extra, so it can run in CI.
 *
 * Usage: pnpm check:i18n
 */
import en from "../src/i18n/locales/en.ts";
import zhCN from "../src/i18n/locales/zh-CN.ts";

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

function main(): void {
  const enKeys = new Set(flattenKeys(en));
  const zhKeys = new Set(flattenKeys(zhCN));

  const missingInZh = [...enKeys].filter((k) => !zhKeys.has(k)).sort();
  const extraInZh = [...zhKeys].filter((k) => !enKeys.has(k)).sort();

  if (missingInZh.length === 0 && extraInZh.length === 0) {
    console.log("i18n completeness check passed (en vs zh-CN).");
    return;
  }

  console.error("i18n completeness check failed (en vs zh-CN):");
  if (missingInZh.length > 0) {
    console.error(`  Missing in zh-CN (${missingInZh.length}):`);
    for (const key of missingInZh) {
      console.error(`    - ${key}`);
    }
  }
  if (extraInZh.length > 0) {
    console.error(`  Extra in zh-CN (${extraInZh.length}):`);
    for (const key of extraInZh) {
      console.error(`    - ${key}`);
    }
  }
  process.exit(1);
}

main();
