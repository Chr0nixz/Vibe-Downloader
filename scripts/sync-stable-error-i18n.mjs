import fs from "node:fs";
import path from "node:path";

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

function buildBlock(locale, messages) {
  const report = reportLocales[locale];
  const lines = ["  errors: {"];
  for (const code of STABLE_ERROR_CODES) {
    const key = camelFromCode(code);
    const raw = messages[code];
    if (!raw) throw new Error(`Missing ${locale} translation for ${code}`);
    lines.push(`    ${key}: ${JSON.stringify(raw)},`);
  }
  lines.push("    report: {");
  for (const [k, v] of Object.entries(report)) {
    lines.push(`      ${k}: ${JSON.stringify(v)},`);
  }
  lines.push("    },");
  lines.push("  },");
  return lines.join("\n");
}

const messageSets = {
  en: STABLE_ERROR_MESSAGES_EN,
  "zh-CN": requireLocalizedSet("zh-CN"),
  "zh-TW": requireLocalizedSet("zh-TW"),
  ja: requireLocalizedSet("ja"),
  ko: requireLocalizedSet("ko"),
  ru: requireLocalizedSet("ru"),
  es: requireLocalizedSet("es"),
};

const root = path.resolve("src/i18n/locales");
let failed = 0;
for (const locale of Object.keys(messageSets)) {
  const file = path.join(root, `${locale}.ts`);
  let text = fs.readFileSync(file, "utf8");
  // Normalize CRLF so the errors-block regex matches on Windows checkouts.
  text = text.replace(/\r\n/g, "\n");
  const block = buildBlock(locale, messageSets[locale]);
  const replaced = text.replace(/ {2}errors: \{[\s\S]*?\n {2}\},/, block);
  if (replaced === text) {
    // Already synced (identical block) or regex miss — distinguish by presence of a new key.
    if (text.includes("tempFileSmallerThanProgress:")) {
      console.log("Unchanged", file, "(already synced)");
      continue;
    }
    console.error("Failed to replace errors block in", file);
    failed += 1;
    continue;
  }
  fs.writeFileSync(file, replaced);
  console.log("Updated", file, "codes=", STABLE_ERROR_CODES.length);
}
if (failed > 0) process.exit(1);
