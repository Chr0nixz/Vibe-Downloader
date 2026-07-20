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
    title: "Export Vibe backup",
    defaultPath: `vibe-backup-${new Date().toISOString().slice(0, 10)}.vibe-backup`,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!destination) return null;
  return createAppBackup(destination);
}

export async function validateSelectedAppBackup(): Promise<BackupValidateResult | null> {
  if (!isTauriRuntime()) {
    log.debug("backup validate skipped outside Tauri");
    return null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: "Validate Vibe backup",
    multiple: false,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return validateAppBackup(selected);
}

export async function restoreSelectedAppBackup(): Promise<BackupRestoreResult | null> {
  if (!isTauriRuntime()) {
    log.debug("backup restore skipped outside Tauri");
    return null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: "Restore Vibe backup",
    multiple: false,
    filters: [{ name: "Vibe Backup", extensions: ["vibe-backup"] }],
  });
  if (!selected || Array.isArray(selected)) return null;
  return restoreAppBackup(selected);
}
