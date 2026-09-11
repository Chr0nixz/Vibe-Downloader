import i18n from "@/i18n";

/**
 * FUN-22: date/time formatting must follow the language chosen in Settings, not
 * the OS locale. Three call sites previously passed `undefined` to
 * `toLocale*String`, so a user running the app in Japanese on an English system
 * still saw English-formatted timestamps.
 */
export type DateTimeStyle = "date" | "time" | "dateTime" | "dateTimeSeconds";

const STYLE_OPTIONS: Record<DateTimeStyle, Intl.DateTimeFormatOptions> = {
  date: { year: "numeric", month: "short", day: "numeric" },
  time: { hour: "2-digit", minute: "2-digit" },
  dateTime: { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" },
  dateTimeSeconds: {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  },
};

// Cache Intl.DateTimeFormat instances — construction costs one to two orders of
// magnitude more than .format(), and these run per task row / per event line.
const formatterCache = new Map<string, Intl.DateTimeFormat>();

function dateTimeFormatter(locale: string, style: DateTimeStyle): Intl.DateTimeFormat {
  const key = `${locale}:${style}`;
  let formatter = formatterCache.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, STYLE_OPTIONS[style]);
    formatterCache.set(key, formatter);
  }
  return formatter;
}

// Clear the cache when the active language changes so month and day names follow
// the new locale instead of the one captured at construction time.
if (i18n && typeof i18n.on === "function") {
  i18n.on("languageChanged", () => formatterCache.clear());
}

/**
 * Formats a timestamp in the active application locale. Unparseable input is
 * returned unchanged so callers can keep printing the raw backend value.
 */
export function formatDateTime(value: string | number | Date, style: DateTimeStyle): string {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return typeof value === "string" ? value : "—";
  return dateTimeFormatter(i18n.language, style).format(date);
}
