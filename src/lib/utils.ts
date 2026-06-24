import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

import i18n from "@/i18n";
import type { Platform } from "./platform";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const BYTE_UNIT_KEYS = [
  "format.byteUnit.b",
  "format.byteUnit.kb",
  "format.byteUnit.mb",
  "format.byteUnit.gb",
  "format.byteUnit.tb",
] as const;

// Cache Intl.NumberFormat instances — they are expensive to construct and
// formatBytes/formatSpeed/formatPercent are called on every progress tick.
const formatterCache = new Map<string, Intl.NumberFormat>();

function numberFormatter(locale: string, fractionDigits: number): Intl.NumberFormat {
  const key = `${locale}:${fractionDigits}`;
  let fmt = formatterCache.get(key);
  if (!fmt) {
    fmt = new Intl.NumberFormat(locale, {
      maximumFractionDigits: fractionDigits,
      minimumFractionDigits: 0,
    });
    formatterCache.set(key, fmt);
  }
  return fmt;
}

// Clear the cache when the active language changes so locale-specific grouping
// (e.g. thousands separators) stays correct after a language switch.
if (i18n && typeof i18n.on === "function") {
  i18n.on("languageChanged", () => formatterCache.clear());
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return `0 ${i18n.t("format.byteUnit.b")}`;
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), BYTE_UNIT_KEYS.length - 1);
  const value = bytes / 1024 ** index;
  const locale = i18n.language;
  const formatted = numberFormatter(locale, index === 0 ? 0 : 1).format(value);
  return `${formatted} ${i18n.t(BYTE_UNIT_KEYS[index])}`;
}

export function formatSpeed(bps: number): string {
  if (bps <= 0) return "—";
  const index = Math.min(Math.floor(Math.log(bps) / Math.log(1024)), BYTE_UNIT_KEYS.length - 1);
  const value = bps / 1024 ** index;
  const locale = i18n.language;
  const formatted = numberFormatter(locale, index === 0 ? 0 : 1).format(value);
  const unit = i18n.t(BYTE_UNIT_KEYS[index]);
  return i18n.t("format.speed", { value: formatted, unit });
}

export function formatEta(downloaded: number, total: number, speedBps: number): string {
  if (total <= 0 || downloaded >= total) return "—";
  if (speedBps <= 0) return "—";
  const seconds = Math.ceil((total - downloaded) / speedBps);
  if (seconds < 60) return i18n.t("format.eta.seconds", { n: seconds });
  if (seconds < 3600) return i18n.t("format.eta.minutes", { n: Math.ceil(seconds / 60) });
  if (seconds < 86400) {
    return i18n.t("format.eta.hours", {
      h: Math.floor(seconds / 3600),
      m: Math.ceil((seconds % 3600) / 60),
    });
  }
  return i18n.t("format.eta.days", {
    d: Math.floor(seconds / 86400),
    h: Math.floor((seconds % 86400) / 3600),
  });
}

export function formatPercent(downloaded: number, total: number): string {
  if (total <= 0) return "—";
  const locale = i18n.language;
  const value = numberFormatter(locale, 1).format(Math.min(100, (downloaded / total) * 100));
  return i18n.t("format.percent", { value });
}

export function sanitizeUrlForDisplay(value: string): string {
  try {
    const url = new URL(value);
    url.username = url.username ? "user" : "";
    url.password = "";
    return url.toString();
  } catch {
    return value.replace(/\/\/([^/@\s]+):([^/@\s]+)@/, "//user@");
  }
}

export function formatShortcut(shortcut: string, platform: Platform): string {
  const mod = platform === "macos" ? "⌘" : "Ctrl";
  return shortcut.replace(/mod\+/gi, `${mod}+`);
}
