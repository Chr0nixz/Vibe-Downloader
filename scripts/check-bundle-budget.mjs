#!/usr/bin/env node
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
/**
 * PERF-10: Aggregate initial-shell bundle sizes and fail if over budget.
 * Does not hard-cap individual vendor chunks — failures print a per-file table.
 */
import { brotliCompressSync, gzipSync } from "node:zlib";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");

export function loadBudget(path = join(root, "scripts/bundle-budget.json")) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function matchAny(name, patterns) {
  return patterns.some((pattern) => new RegExp(pattern).test(name));
}

export function classifyAsset(name, config) {
  if (matchAny(name, config.initialShell.exclude)) {
    return "deferred";
  }
  if (matchAny(name, config.initialShell.include)) {
    return "initial";
  }
  // Unlisted hashed chunks: treat JS/CSS as deferred unless clearly shell.
  if (/\.(js|css)$/.test(name)) {
    return "deferred";
  }
  return "ignore";
}

export function measureFile(bytes) {
  return {
    raw: bytes.length,
    gzip: gzipSync(bytes).length,
    brotli: brotliCompressSync(bytes).length,
  };
}

export function summarizeAssets(files, config) {
  const initialJs = [];
  const initialCss = [];
  const deferred = [];
  for (const file of files) {
    const kind = classifyAsset(file.name, config);
    if (kind === "ignore") continue;
    const sizes = measureFile(file.bytes);
    const row = { name: file.name, ...sizes };
    if (kind === "deferred") {
      deferred.push(row);
      continue;
    }
    if (file.name.endsWith(".css")) {
      initialCss.push(row);
    } else {
      initialJs.push(row);
    }
  }
  const sum = (rows, key) => rows.reduce((acc, row) => acc + row[key], 0);
  return {
    initialJs,
    initialCss,
    deferred,
    totals: {
      initialShellJsRawBytes: sum(initialJs, "raw"),
      initialShellJsGzipBytes: sum(initialJs, "gzip"),
      initialShellJsBrotliBytes: sum(initialJs, "brotli"),
      initialShellCssGzipBytes: sum(initialCss, "gzip"),
    },
  };
}

export function checkBudgets(totals, budgets) {
  const failures = [];
  for (const [key, limit] of Object.entries(budgets)) {
    const actual = totals[key];
    if (typeof actual !== "number") continue;
    if (actual > limit) {
      failures.push({ key, actual, limit });
    }
  }
  return failures;
}

function formatKb(bytes) {
  return `${(bytes / 1024).toFixed(1)} kB`;
}

function printTable(title, rows) {
  console.log(`\n${title}`);
  for (const row of rows.sort((a, b) => b.gzip - a.gzip)) {
    console.log(
      `  ${row.name.padEnd(42)} raw=${formatKb(row.raw).padStart(8)} gzip=${formatKb(row.gzip).padStart(8)} brotli=${formatKb(row.brotli).padStart(8)}`,
    );
  }
}

export function main() {
  const config = loadBudget();
  const distDir = join(root, config.distDir);
  if (!existsSync(distDir)) {
    console.error(`Missing ${config.distDir}. Run pnpm build first.`);
    process.exit(1);
  }
  const files = readdirSync(distDir)
    .filter((name) => /\.(js|css)$/.test(name))
    .map((name) => ({
      name,
      bytes: readFileSync(join(distDir, name)),
    }));
  const summary = summarizeAssets(files, config);
  printTable("Initial shell JS", summary.initialJs);
  printTable("Initial shell CSS", summary.initialCss);
  printTable("Deferred (not gated)", summary.deferred.slice(0, 12));

  console.log("\nTotals");
  for (const [key, value] of Object.entries(summary.totals)) {
    const limit = config.budgets[key];
    const mark = value <= limit ? "OK" : "FAIL";
    console.log(`  ${key}: ${formatKb(value)} / budget ${formatKb(limit)} [${mark}]`);
  }

  const failures = checkBudgets(summary.totals, config.budgets);
  if (failures.length > 0) {
    console.error("\nPERF-10 bundle budget exceeded:");
    for (const failure of failures) {
      console.error(`  ${failure.key}: ${failure.actual} > ${failure.limit} bytes`);
    }
    process.exit(1);
  }
  console.log("\nPERF-10 bundle budget OK");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
