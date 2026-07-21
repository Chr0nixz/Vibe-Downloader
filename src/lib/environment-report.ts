//! Pure helpers for formatting the Environment health report for clipboard copy.

import type { EnvironmentHealthReport, EnvironmentHealthStatus } from "@/generated/bindings";
import type { UpdateStatus } from "@/stores/updater-store";

export type EnvironmentUpdaterSnapshot = {
  currentVersion: string | null;
  updateVersion: string | null;
  status: UpdateStatus;
  error: string | null;
};

export function formatEnvironmentReport(report: EnvironmentHealthReport, updater: EnvironmentUpdaterSnapshot): string {
  const checkedAt = formatCheckedAt(report.checkedAtMs);
  const lines: string[] = [
    "Vibe Downloader — Environment diagnostics",
    `Checked at: ${checkedAt}`,
    `App version: ${report.appVersion}`,
    `Platform: ${report.platform}`,
    "",
    "Checks:",
  ];

  for (const item of report.items) {
    lines.push(`- [${statusLabel(item.status)}] ${item.id}: ${item.summary}`);
    if (item.detail) {
      lines.push(`  detail: ${sanitizeDetail(item.detail)}`);
    }
  }

  lines.push("");
  lines.push("Updater:");
  lines.push(`- status: ${updater.status}`);
  if (updater.currentVersion) {
    lines.push(`- current: ${updater.currentVersion}`);
  }
  if (updater.updateVersion) {
    lines.push(`- available: ${updater.updateVersion}`);
  }
  if (updater.error) {
    lines.push(`- error: ${sanitizeDetail(updater.error)}`);
  }

  lines.push("");
  lines.push("Note: report excludes passwords, cookies, and other secrets.");
  return lines.join("\n");
}

function statusLabel(status: EnvironmentHealthStatus): string {
  return status.toUpperCase();
}

function formatCheckedAt(checkedAtMs: string): string {
  const ms = Number(checkedAtMs);
  if (!Number.isFinite(ms) || ms <= 0) return checkedAtMs;
  try {
    return new Date(ms).toISOString();
  } catch {
    return checkedAtMs;
  }
}

/** Strip obvious credential-looking query fragments from free-form detail text. */
function sanitizeDetail(detail: string): string {
  return detail
    .replace(/(password|passwd|pwd|token|cookie|authorization)\s*[:=]\s*\S+/gi, "$1=[redacted]")
    .replace(/:[^/@\s]+@/g, ":[redacted]@");
}
