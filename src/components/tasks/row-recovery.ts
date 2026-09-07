import type { RecoveryAction } from "@/generated/bindings";
import { recoveryActionsForError } from "@/lib/errors";
import type { Task } from "@/types/task";

type RecoveryTask = Pick<Task, "status" | "errorMessage" | "recoveryActions">;

/** Prefer persisted actions; fall back to the error-code map used by InlineRecovery. */
export function recoveryActionsForTask(task: RecoveryTask): RecoveryAction[] {
  if (task.recoveryActions.length > 0) return task.recoveryActions;
  return recoveryActionsForError(task.errorMessage ?? "");
}

/**
 * Failed / needs-attention rows that already expose InlineRecovery should not
 * also offer Resume or a second Retry — those contradict Restart / Save as.
 */
export function hasInlineRecovery(task: RecoveryTask): boolean {
  if (task.status !== "failed" && task.status !== "needs_attention") return false;
  if (!task.errorMessage) return false;
  return recoveryActionsForTask(task).length > 0;
}

export function rowTransferMode(task: RecoveryTask): "pause" | "resume" | "hidden" {
  if (task.status === "completed" || task.status === "needs_attention") return "hidden";
  if (task.status === "failed" && hasInlineRecovery(task)) return "hidden";
  if (task.status === "paused" || task.status === "failed" || task.status === "waiting_network") return "resume";
  return "pause";
}

export function rowShowsRetry(task: RecoveryTask): boolean {
  return task.status === "failed" && !hasInlineRecovery(task);
}
