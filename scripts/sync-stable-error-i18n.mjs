import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// Load TS module via vitest/tsx isn't available; duplicate the minimal maps here
// by importing the compiled source through dynamic import of the .ts via jiti-less parse.
// Instead, read stable-error-codes.ts as text and eval the EN messages object via a tiny transform.

const stablePath = path.resolve("src/lib/stable-error-codes.ts");
const stableSrc = fs.readFileSync(stablePath, "utf8");

function extractArray(name) {
  const match = stableSrc.match(new RegExp(`export const ${name} = \\[([\\s\\S]*?)\\] as const;`));
  if (!match) throw new Error(`Missing ${name}`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

function extractEnMessages() {
  const match = stableSrc.match(/export const STABLE_ERROR_MESSAGES_EN[^=]*= \{([\s\S]*?)\n\};/);
  if (!match) throw new Error("Missing STABLE_ERROR_MESSAGES_EN");
  const obj = {};
  for (const m of match[1].matchAll(/^\s*([a-z0-9_]+):\s*"((?:\\.|[^"\\])*)"/gm)) {
    obj[m[1]] = m[2].replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
  return obj;
}

const STABLE_ERROR_CODES = extractArray("STABLE_ERROR_CODES");
const STABLE_ERROR_MESSAGES_EN = extractEnMessages();

function camelFromCode(code) {
  return code.replace(/_([a-z])/g, (_, ch) => ch.toUpperCase());
}

// ENG-10: the mechanism ("cause") copy used to be maintained by hand inside
// each locale's errors block. Because this script replaces that block
// wholesale, a hand-written `cause` sub-block disappears on the next sync —
// which is exactly how all seven locales lost theirs. Cause copy now lives in
// its own data file and is generated alongside the stable codes, so the
// replacement can no longer drop it.
const localizedCauses = JSON.parse(fs.readFileSync(path.resolve("scripts/stable-error-causes.json"), "utf8"));
const CAUSE_KEYS = Object.keys(localizedCauses.en).sort();

function requireCauseSet(locale) {
  const causes = localizedCauses[locale];
  if (!causes) throw new Error(`Missing cause copy for ${locale}`);
  const missing = CAUSE_KEYS.filter((key) => !causes[key]);
  if (missing.length > 0) {
    throw new Error(`${locale} is missing ${missing.length} cause translations: ${missing.join(", ")}`);
  }
  return causes;
}

const localizedMessages = JSON.parse(fs.readFileSync(path.resolve("scripts/stable-error-messages.json"), "utf8"));

function requireLocalizedSet(locale) {
  const messages = localizedMessages[locale];
  if (!messages) throw new Error(`Missing localized error set for ${locale}`);
  const missing = STABLE_ERROR_CODES.filter((code) => !messages[code]);
  if (missing.length > 0) {
    throw new Error(`${locale} is missing ${missing.length} error translations: ${missing.join(", ")}`);
  }
  const stillEnglish = STABLE_ERROR_CODES.filter((code) => messages[code] === STABLE_ERROR_MESSAGES_EN[code]);
  if (stillEnglish.length > 0) {
    throw new Error(`${locale} still copies English for: ${stillEnglish.join(", ")}`);
  }
  return messages;
}

const reportLocales = {
  en: {
    taskId: "Task ID",
    url: "URL",
    code: "Code",
    message: "Message",
    recoverable: "Recoverable",
    actions: "Actions",
  },
  "zh-CN": {
    taskId: "任务 ID",
    url: "URL",
    code: "错误码",
    message: "消息",
    recoverable: "可恢复",
    actions: "动作",
  },
  "zh-TW": {
    taskId: "任務 ID",
    url: "URL",
    code: "錯誤碼",
    message: "訊息",
    recoverable: "可恢復",
    actions: "動作",
  },
  ja: {
    taskId: "タスク ID",
    url: "URL",
    code: "コード",
    message: "メッセージ",
    recoverable: "回復可能",
    actions: "アクション",
  },
  ko: {
    taskId: "작업 ID",
    url: "URL",
    code: "코드",
    message: "메시지",
    recoverable: "복구 가능",
    actions: "동작",
  },
  ru: {
    taskId: "ID задачи",
    url: "URL",
    code: "Код",
    message: "Сообщение",
    recoverable: "Восстановимо",
    actions: "Действия",
  },
  es: {
    taskId: "ID de tarea",
    url: "URL",
    code: "Código",
    message: "Mensaje",
    recoverable: "Recuperable",
    actions: "Acciones",
  },
};

/** Generates the errors block from a message set plus its report sub-block. */
function buildBlockFrom(messages, report, codes, causes) {
  const lines = ["  errors: {"];
  for (const code of codes) {
    const key = camelFromCode(code);
    const raw = messages[code];
    if (!raw) throw new Error(`Missing translation for ${code}`);
    lines.push(`    ${key}: ${JSON.stringify(raw)},`);
  }
  if (causes && Object.keys(causes).length > 0) {
    lines.push("    cause: {");
    for (const key of Object.keys(causes).sort()) {
      lines.push(`      ${key}: ${JSON.stringify(causes[key])},`);
    }
    lines.push("    },");
  }
  lines.push("    report: {");
  for (const [k, v] of Object.entries(report)) {
    lines.push(`      ${k}: ${JSON.stringify(v)},`);
  }
  lines.push("    },");
  lines.push("  },");
  return lines.join("\n");
}

/** Matches the `errors: { ... },` block nested one level inside a locale file. */
export const ERRORS_BLOCK_PATTERN = / {2}errors: \{[\s\S]*?\n {2}\},/;

/** Keys present in the existing errors block, or null when the block is absent. */
export function parseErrorsBlockKeys(text) {
  const match = text.match(ERRORS_BLOCK_PATTERN);
  if (!match) return null;
  return [...match[0].matchAll(/^\s{4}([A-Za-z0-9_]+):/gm)].map((m) => m[1]);
}

/**
 * Replaces the locale's errors block, structurally distinguishing the two
 * ways `String.replace` can be a no-op.
 *
 * The old code looked for one sentinel key (`tempFileSmallerThanProgress`)
 * to tell "already synced" from "regex missed the block". Renaming or
 * deleting that code turned perfectly healthy locales into hard failures.
 * This version asks a structural question instead: does an errors block
 * exist at all, and does it still carry keys derived from STABLE_ERROR_CODES?
 *
 * - `updated`: the block changed (or is absent and nothing matched).
 * - `unchanged`: the block exists and already matches, or the code list moved
 *   on and the block still carries the older derived keys.
 * - `failed`: no errors block could be located — the block regex itself
 *   drifted, which is the only condition that must fail loudly.
 */
export function syncLocaleErrorsBlock({ text, messages, report, codes = STABLE_ERROR_CODES, causes = null }) {
  const normalized = text.replace(/\r\n/g, "\n");
  const block = buildBlockFrom(messages, report, codes, causes);
  const replaced = normalized.replace(ERRORS_BLOCK_PATTERN, block);
  if (replaced !== normalized) {
    return { status: "updated", text: replaced };
  }
  const existingKeys = parseErrorsBlockKeys(normalized);
  if (existingKeys === null) {
    return { status: "failed", text: normalized };
  }
  const derived = new Set(codes.map(camelFromCode));
  const carriesDerivedKeys = existingKeys.some((key) => derived.has(key));
  return carriesDerivedKeys ? { status: "unchanged", text: normalized } : { status: "failed", text: normalized };
}

function main() {
  const messageSets = {
    en: STABLE_ERROR_MESSAGES_EN,
    "zh-CN": requireLocalizedSet("zh-CN"),
    "zh-TW": requireLocalizedSet("zh-TW"),
    ja: requireLocalizedSet("ja"),
    ko: requireLocalizedSet("ko"),
    ru: requireLocalizedSet("ru"),
    es: requireLocalizedSet("es"),
  };

  const biome = fileURLToPath(import.meta.resolve("@biomejs/biome/bin/biome"));
  const root = path.resolve("src/i18n/locales");
  let failed = 0;
  for (const locale of Object.keys(messageSets)) {
    const file = path.join(root, `${locale}.ts`);
    const text = fs.readFileSync(file, "utf8");
    const result = syncLocaleErrorsBlock({
      text,
      messages: messageSets[locale],
      report: reportLocales[locale],
      causes: requireCauseSet(locale),
    });
    if (result.status === "failed") {
      console.error("Failed to replace errors block in", file);
      failed += 1;
      continue;
    }
    // Compare formatted output before writing so wrapping cannot dirty an already synced locale.
    const formatted = execFileSync(process.execPath, [biome, "format", "--stdin-file-path", file], {
      input: result.text,
      encoding: "utf8",
    });
    if (formatted === text.replace(/\r\n/g, "\n")) {
      console.log("Unchanged", file, "(already synced)");
      continue;
    }
    fs.writeFileSync(file, formatted);
    console.log("Updated", file, "codes=", STABLE_ERROR_CODES.length);
  }
  if (failed > 0) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
