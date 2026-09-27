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

function numberFormatter(locale: string, fractionDigits: number, fixed = false): Intl.NumberFormat {
  const key = `${locale}:${fractionDigits}:${fixed ? "fixed" : "trim"}`;
  let fmt = formatterCache.get(key);
  if (!fmt) {
    fmt = new Intl.NumberFormat(locale, {
      maximumFractionDigits: fractionDigits,
      minimumFractionDigits: fixed ? fractionDigits : 0,
    });
    formatterCache.set(key, fmt);
  }
  return fmt;
}

/** `fixed` keeps the decimal place even when it is zero ("371.0 MB"). Live
 * values use it so a label does not change width every time a byte count or
 * speed crosses a whole unit; static labels keep the shorter trimmed form. */
export interface NumberFormatOptions {
  fixed?: boolean;
}

// Clear the cache when the active language changes so locale-specific grouping
// (e.g. thousands separators) stays correct after a language switch.
if (i18n && typeof i18n.on === "function") {
  i18n.on("languageChanged", () => formatterCache.clear());
}

export function formatBytes(bytes: number, options?: NumberFormatOptions): string {
  if (bytes <= 0) return `0 ${i18n.t("format.byteUnit.b")}`;
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), BYTE_UNIT_KEYS.length - 1);
  const value = bytes / 1024 ** index;
  const locale = i18n.language;
  const formatted = numberFormatter(locale, index === 0 ? 0 : 1, options?.fixed).format(value);
  return `${formatted} ${i18n.t(BYTE_UNIT_KEYS[index])}`;
}

export function formatSpeed(bps: number, options?: NumberFormatOptions): string {
  if (bps <= 0) return "—";
  const index = Math.min(Math.floor(Math.log(bps) / Math.log(1024)), BYTE_UNIT_KEYS.length - 1);
  const value = bps / 1024 ** index;
  const locale = i18n.language;
  const formatted = numberFormatter(locale, index === 0 ? 0 : 1, options?.fixed).format(value);
  const unit = i18n.t(BYTE_UNIT_KEYS[index]);
  return i18n.t("format.speed", { value: formatted, unit });
}

/** Speed label for an active transfer that is moving 0 B/s right now (stalled).
 * formatSpeed renders "—" for <=0, which is the right placeholder where no
 * speed applies (paused rows, idle totals) but wrong where the stall itself is
 * the diagnostic fact the row should surface. */
export function formatStalledSpeed(): string {
  const unit = i18n.t(BYTE_UNIT_KEYS[1]);
  return i18n.t("format.speed", { value: numberFormatter(i18n.language, 0).format(0), unit });
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

export function formatPercent(
  downloaded: number,
  total: number,
  maxPercent = 100,
  options?: NumberFormatOptions,
): string {
  if (total <= 0) return "—";
  const locale = i18n.language;
  const ceiling = Number.isFinite(maxPercent) ? Math.min(100, Math.max(0, maxPercent)) : 100;
  const value = numberFormatter(locale, 1, options?.fixed).format(Math.min(ceiling, (downloaded / total) * 100));
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

/** Shortcut hint for surfaces that are not handed the platform (context menus
 * portal out of the shell). AppShell mirrors the detected platform onto
 * `<html data-platform>`, so this matches what the key handler listens for. */
export function formatShortcutForDocument(shortcut: string): string {
  const platform = typeof document === "undefined" ? "unknown" : document.documentElement.dataset.platform;
  return formatShortcut(shortcut, platform === "macos" ? "macos" : "windows");
}
