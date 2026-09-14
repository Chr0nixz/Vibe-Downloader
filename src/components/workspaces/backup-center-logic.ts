//! Pure logic for the Backup & Migration Center (feature proposal §3.7).
//!
//! Mirrors the storage/recovery center pattern: typed i18n key tables that
//! `check:i18n` cannot see as literals get a locale-walk test in
//! `backup-center-logic.test.ts`, and every struct the page renders maps
//! through a `Record<Union, TranslationKey>` table here.

import type { TranslationKey } from "@/i18n";

export type NotExportedItem = "proxyPassword" | "credentials" | "downloadedFiles" | "btState";

/** §3.7 "explicitly marked as not exported" list, in display order. */
export const NOT_EXPORTED_ITEMS: readonly NotExportedItem[] = [
  "proxyPassword",
  "credentials",
  "downloadedFiles",
  "btState",
] as const satisfies readonly NotExportedItem[];

export const NOT_EXPORTED_KEYS: Record<NotExportedItem, TranslationKey> = {
  proxyPassword: "backupCenter.notExported.proxyPassword",
  credentials: "backupCenter.notExported.credentials",
  downloadedFiles: "backupCenter.notExported.downloadedFiles",
  btState: "backupCenter.notExported.btState",
};

export type ContentsField =
  | "tasksTotal"
  | "tasksCompleted"
  | "tasksFailed"
  | "classificationRules"
  | "siteRules"
  | "tasksWithChecksums"
  | "tasksWithCredentials"
  | "tasksWithRequestHeaders"
  | "settingsKeys"
  | "taskEvents";

/** Label key per inventory counter, in display order. */
export const CONTENTS_FIELD_KEYS: Record<ContentsField, TranslationKey> = {
  tasksTotal: "backupCenter.contents.tasksTotal",
  tasksCompleted: "backupCenter.contents.tasksCompleted",
  tasksFailed: "backupCenter.contents.tasksFailed",
  classificationRules: "backupCenter.contents.classificationRules",
  siteRules: "backupCenter.contents.siteRules",
  tasksWithChecksums: "backupCenter.contents.tasksWithChecksums",
  tasksWithCredentials: "backupCenter.contents.tasksWithCredentials",
  tasksWithRequestHeaders: "backupCenter.contents.tasksWithRequestHeaders",
  settingsKeys: "backupCenter.contents.settingsKeys",
  taskEvents: "backupCenter.contents.taskEvents",
};

export const CONTENTS_FIELD_ORDER: readonly ContentsField[] = [
  "tasksTotal",
  "tasksCompleted",
  "tasksFailed",
  "classificationRules",
  "siteRules",
  "tasksWithChecksums",
  "tasksWithCredentials",
  "tasksWithRequestHeaders",
  "settingsKeys",
  "taskEvents",
] as const satisfies readonly ContentsField[];

export type SubsetField = "tasks" | "rules" | "settings";

export const SUBSET_FIELD_KEYS: Record<SubsetField, TranslationKey> = {
  tasks: "backupCenter.subset.tasks",
  rules: "backupCenter.subset.rules",
  settings: "backupCenter.subset.settings",
};

export const SUBSET_HINT_KEYS: Record<SubsetField, TranslationKey> = {
  tasks: "backupCenter.subset.tasksHint",
  rules: "backupCenter.subset.rulesHint",
  settings: "backupCenter.subset.settingsHint",
};

export const SUBSET_FIELD_ORDER: readonly SubsetField[] = ["tasks", "rules", "settings"] as const;

/** One "what to reconfigure" bullet derived from the post-restore report. */
export interface ReportEntry {
  key: TranslationKey;
  values?: Record<string, string | number>;
}

/**
 * Build the report bullet list (§3.7 "after restore, list what needs to be
 * reconfigured"). Ordered by user impact: credentials first, then proxy,
 * machine-scrubbed settings, missing folders, rollback pointer.
 */
export function reportEntries(report: {
  tasksWithCredentials: number;
  tasksWithPerTaskProxy: number;
  globalProxyNeedsReentry: boolean;
  ffmpegWasConfigured: boolean;
  completionActionReset: boolean;
  missingSaveDirs: string[];
  missingSaveDirsTotal: number;
  preRestoreBackupPath: string | null;
  backupCreatedAt: string | null;
}): ReportEntry[] {
  const entries: ReportEntry[] = [];
  if (report.tasksWithCredentials > 0) {
    entries.push({
      key: "backupCenter.report.credentials",
      values: { count: report.tasksWithCredentials },
    });
  }
  if (report.globalProxyNeedsReentry) {
    entries.push({ key: "backupCenter.report.proxyGlobal" });
  }
  if (report.tasksWithPerTaskProxy > 0) {
    entries.push({
      key: "backupCenter.report.proxy",
      values: { count: report.tasksWithPerTaskProxy },
    });
  }
  if (report.ffmpegWasConfigured) {
    entries.push({ key: "backupCenter.report.ffmpeg" });
  }
  if (report.completionActionReset) {
    entries.push({ key: "backupCenter.report.completion" });
  }
  if (report.missingSaveDirs.length > 0) {
    const shown = report.missingSaveDirs.join(", ");
    const extra = report.missingSaveDirsTotal - report.missingSaveDirs.length;
    const dirs = extra > 0 ? `${shown} +${extra}` : shown;
    entries.push({ key: "backupCenter.report.missingDirs", values: { dirs } });
  }
  if (report.preRestoreBackupPath) {
    entries.push({
      key: "backupCenter.report.rollback",
      values: { path: report.preRestoreBackupPath },
    });
  }
  return entries;
}

/** Severity of the pre-restore disk check for styling and the check line. */
export type DiskCheckLevel = "ok" | "low" | "unknown";

export function diskCheckLevel(freeBytes: string | null, requiredBytes: string): DiskCheckLevel {
  if (freeBytes === null) return "unknown";
  return BigInt(freeBytes) >= BigInt(requiredBytes) ? "ok" : "low";
}

/** Byte counts cross the IPC boundary as decimal strings (Specta forbids u64). */
export function bytesToNumber(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}
