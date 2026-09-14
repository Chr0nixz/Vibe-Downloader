import i18n, { type TranslationKey } from "@/i18n";
import { createLogger } from "@/lib/logger";
import { isTauriRuntime } from "@/lib/runtime";
import {
  type BackupCreateResult,
  type BackupRestoreResult,
  type BackupValidateResult,
  createAppBackup,
  restoreAppBackup,
  validateAppBackup,
} from "@/lib/tauri";

const log = createLogger("backup");

export type { BackupCreateResult, BackupRestoreResult, BackupValidateResult };

export async function exportAppBackup(): Promise<BackupCreateResult | null> {
  if (!isTauriRuntime()) {
    log.debug("backup export skipped outside Tauri");
    return null;
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const destination = await save({
    // OS dialog chrome follows the app language, like every in-app surface does.
    title: i18n.t("settings.dataBackupExportTitle"),
    defaultPath: `vibe-backup-${new Date().toISOString().slice(0, 10)}.vibe-backup`,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!destination) return null;
  return createAppBackup(destination);
}

export async function validateSelectedAppBackup(
  dialogTitleKey: TranslationKey = "settings.dataBackupValidateTitle",
): Promise<BackupValidateResult | null> {
  if (!isTauriRuntime()) {
    log.debug("backup validate skipped outside Tauri");
    return null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: i18n.t(dialogTitleKey),
    multiple: false,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return validateAppBackup(selected);
}

/// Pick an archive for the Backup Center, where the same file feeds either a
/// whole restore or a subset restore.
export async function pickBackupFile(dialogTitleKey: TranslationKey): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: i18n.t(dialogTitleKey),
    multiple: false,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return selected;
}

/// Pick the migration target root (§3.7 path remap). Returns null on cancel.
export async function pickRemapRoot(dialogTitleKey: TranslationKey): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: i18n.t(dialogTitleKey),
    directory: true,
    multiple: false,
  });
  if (!selected || Array.isArray(selected)) return null;
  return selected;
}

export async function restoreSelectedAppBackup(): Promise<BackupRestoreResult | null> {
  if (!isTauriRuntime()) {
    log.debug("backup restore skipped outside Tauri");
    return null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: i18n.t("settings.dataBackupRestoreTitle"),
    multiple: false,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return restoreAppBackup(selected);
}
