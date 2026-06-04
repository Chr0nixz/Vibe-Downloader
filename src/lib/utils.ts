import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

import type { Platform } from "./platform";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

export function formatSpeed(bps: number): string {
  if (bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

export function formatEta(
  downloaded: number,
  total: number,
  speedBps: number,
): string {
  if (total <= 0 || downloaded >= total) return "—";
  if (speedBps <= 0) return "—";
  const seconds = Math.ceil((total - downloaded) / speedBps);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.ceil((seconds % 3600) / 60)}m`;
}

export function formatPercent(downloaded: number, total: number): string {
  if (total <= 0) return "—";
  return `${Math.min(100, (downloaded / total) * 100).toFixed(1)}%`;
}

export function formatShortcut(shortcut: string, platform: Platform): string {
  const mod = platform === "macos" ? "⌘" : "Ctrl";
  return shortcut.replace(/mod\+/gi, `${mod}+`);
}
