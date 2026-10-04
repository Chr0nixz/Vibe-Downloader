import type { TFunction } from "i18next";
import type { RecoveryAction } from "@/generated/bindings";
import { parseAppError, recoveryActionsForError } from "@/lib/errors";
import type { Task } from "@/types/task";

type RecoveryTask = Pick<Task, "status" | "supportsResume"> &
  Partial<Pick<Task, "errorMessage" | "recoveryActions">> &
  Partial<Pick<Task, "errorCode">>;

/** The BT metadata handoff uses the existing navigation action, but its
 * destination is the task's file-selection panel rather than the source URL.
 * Keep this mapping tied to the stable code so other `check_url` recoveries
 * retain their normal meaning. */
export function torrentFileSelectionRequired(task: Partial<Pick<Task, "errorCode" | "errorMessage">>): boolean {
  return (task.errorCode ?? parseAppError(task.errorMessage)?.code) === "bt_file_selection_required";
}

export function recoveryActionLabel(task: RecoveryTask, action: RecoveryAction, t: TFunction): string {
  if (action === "check_url" && torrentFileSelectionRequired(task)) {
    return t("newDownload.chooseFile");
  }
  return t(`recovery.${action}`);
}

const RESTART_REQUIRED_CODES = new Set([
  "remote_changed",
  "resume_unavailable",
  "resume_mismatch",
  "temp_file_missing",
  "temp_file_smaller_than_progress",
]);

export type ResumeVerdict = "available" | "restart_required" | "not_applicable";

/**
 * One answer to the question every transfer surface asks: can the current
 * bytes continue safely? Stored error codes outrank probe capability flags;
 * a server can revoke range support after the original probe.
 */
export function resumeVerdict(task: RecoveryTask): ResumeVerdict {
  if (task.status !== "paused" && task.status !== "waiting_network") {
    return "not_applicable";
  }
  const code = task.errorCode ?? parseAppError(task.errorMessage ?? null)?.code ?? null;
  if (code && RESTART_REQUIRED_CODES.has(code)) return "restart_required";
  if (task.supportsResume === false) return "restart_required";
  return "available";
}

export type TransferAction = "pause" | "resume" | "retry";

/**
 * Pausing an active task with bytes on disk is destructive when the source
 * cannot resume. Every pause surface uses this same verdict before dispatching
 * the command, so a warning in the row cannot drift from the actual action.
 */
export function pauseWouldDiscardProgress(task: Pick<Task, "status" | "supportsResume" | "downloadedBytes">): boolean {
  return (
    (task.status === "downloading" || task.status === "retrying" || task.status === "queued") &&
    task.downloadedBytes > 0 &&
    !task.supportsResume
  );
}

/** Shared action gate used by rows, details, palette and bulk dispatch. */
export function allowedTransferActions(task: RecoveryTask): TransferAction[] {
  if (task.status === "downloading" || task.status === "retrying" || task.status === "queued") return ["pause"];
  if (task.status === "paused" || task.status === "waiting_network") {
    return resumeVerdict(task) === "available" ? ["resume"] : [];
  }
  if (task.status === "failed" || task.status === "needs_attention") {
    const code = task.errorCode ?? parseAppError(task.errorMessage ?? null)?.code ?? null;
    if (code && (RESTART_REQUIRED_CODES.has(code) || code === "final_path_conflict")) return [];
    return ["retry"];
  }
  return [];
}

/** Prefer persisted actions; fall back to the error-code map used by InlineRecovery. */
export function recoveryActionsForTask(task: RecoveryTask): RecoveryAction[] {
  if (task.recoveryActions && task.recoveryActions.length > 0) return task.recoveryActions;
  return recoveryActionsForError(task.errorMessage ?? "");
}

/** File-location access is already available in the row's More menu. */
export function inlineRecoveryActionsForTask(task: RecoveryTask): RecoveryAction[] {
  return recoveryActionsForTask(task).filter((action) => action !== "open_folder");
}

/**
 * Failed / needs-attention rows that already expose InlineRecovery should not
 * also offer Resume or a second Retry — those contradict Restart / Save as.
 */
export function hasInlineRecovery(task: RecoveryTask): boolean {
  if (task.status !== "failed" && task.status !== "needs_attention") return false;
  if (!task.errorMessage) return false;
  return inlineRecoveryActionsForTask(task).length > 0;
}

/**
 * The one "get this task moving again" action, shared by Mod+R, the palette
 * and the context menu so no keyboard path dead-ends where the row offers a
 * button. A plain retry wins when the engine can reuse the bytes on disk;
 * otherwise it is the first fix of the recovery playbook, which for a lost
 * resume is Restart. Restart still goes through its cost confirmation because
 * callers dispatch it like the row's banner does. Null when nothing applies.
 */
export function primaryRecoveryAction(task: RecoveryTask): RecoveryAction | null {
  if (hasInlineRecovery(task)) return inlineRecoveryActionsForTask(task)[0] ?? null;
  if (allowedTransferActions(task).includes("retry")) return "retry";
  return null;
}

export function rowTransferMode(task: RecoveryTask): "pause" | "resume" | "hidden" {
  const action = allowedTransferActions(task)[0];
  return action === "pause" ? "pause" : action === "resume" ? "resume" : "hidden";
}

export function rowShowsRetry(task: RecoveryTask): boolean {
  return allowedTransferActions(task).includes("retry") && !hasInlineRecovery(task);
}
