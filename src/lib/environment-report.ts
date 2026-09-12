//! Pure helpers for formatting the Environment health report for clipboard copy.

import type { TFunction } from "i18next";

import type { EnvironmentHealthReport, EnvironmentHealthStatus } from "@/generated/bindings";
import { formatEnvironmentDetail, formatEnvironmentText } from "@/lib/environment-text";
import type { UpdateStatus } from "@/stores/updater-store";

export type EnvironmentUpdaterSnapshot = {
  currentVersion: string | null;
  updateVersion: string | null;
  status: UpdateStatus;
  error: string | null;
};

/**
 * FUN-28: the report follows the app language like every other surface. Status
 * codes and ids stay machine-facing; only the labels and the item copy are
 * localized.
 */
export function formatEnvironmentReport(
  report: EnvironmentHealthReport,
  updater: EnvironmentUpdaterSnapshot,
  t: TFunction,
): string {
  const checkedAt = formatCheckedAt(report.checkedAtMs);
  const lines: string[] = [
    t("environment.report.title"),
    t("environment.report.checkedAt", { time: checkedAt }),
    t("environment.report.appVersion", { version: report.appVersion }),
    t("environment.report.platform", { platform: report.platform }),
    "",
    t("environment.report.checks"),
  ];

  for (const item of report.items) {
    lines.push(
      t("environment.report.itemLine", {
        status: statusLabel(item.status),
        id: item.id,
        summary: formatEnvironmentText(item.summary, t),
      }),
    );
    const detail = formatEnvironmentDetail(item.detail, t);
    if (detail) {
      lines.push(`  ${t("environment.report.detailLabel")} ${sanitizeDetail(detail)}`);
    }
  }

  lines.push("");
  lines.push(t("environment.report.updater"));
  lines.push(t("environment.report.updaterStatus", { status: updater.status }));
  if (updater.currentVersion) {
    lines.push(t("environment.report.updaterCurrent", { version: updater.currentVersion }));
  }
  if (updater.updateVersion) {
    lines.push(t("environment.report.updaterAvailable", { version: updater.updateVersion }));
  }
  if (updater.error) {
    lines.push(t("environment.report.updaterError", { error: sanitizeDetail(updater.error) }));
  }

  lines.push("");
  lines.push(t("environment.report.note"));
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
