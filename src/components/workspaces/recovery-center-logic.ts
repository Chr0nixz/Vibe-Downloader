import type { RecoveryAction, TaskFailureCategory } from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";
import { parseAppError, recoveryActionsForError } from "@/lib/errors";
import type { Task } from "@/types/task";

// Recovery Center grouping (feature proposal §3.3): failed / needs-attention
// tasks are grouped by the *concern the user has to act on* (auth, proxy,
// disk space, ...), not by the raw failure category. The tables below map
// the backend taxonomy (TaskFailureCategory + stable error codes) onto those
// concerns; keys are typed so a typo is a compile error.

export type RecoveryConcern = "auth" | "proxy" | "disk" | "remoteChanged" | "resume" | "protocol" | "http" | "other";

export const RECOVERY_CONCERNS: RecoveryConcern[] = [
  "auth",
  "proxy",
  "disk",
  "remoteChanged",
  "resume",
  "protocol",
  "http",
  "other",
];

const RESTART_REQUIRED_CODES = new Set([
  "remote_changed",
  "resume_unavailable",
  "temp_file_missing",
  "temp_file_smaller_than_progress",
]);

const CONCERN_BY_CATEGORY: Record<TaskFailureCategory, RecoveryConcern> = {
  auth: "auth",
  proxy: "proxy",
  disk_write: "disk",
  remote_changed: "remoteChanged",
  resume_unavailable: "resume",
  temp_file: "resume",
  hls: "protocol",
  dash: "protocol",
  metalink: "protocol",
  bt: "protocol",
  ftp: "protocol",
  sftp: "protocol",
  webdav: "protocol",
  http: "http",
  schedule: "other",
  other: "other",
};

/** Codes whose concern differs from their category mapping. */
const CONCERN_BY_CODE: Record<string, RecoveryConcern> = {
  // Stored-credential failures are an auth concern even though the
  // category maps them elsewhere.
  task_credentials_unavailable: "auth",
  task_credentials_encrypt_failed: "auth",
  task_credentials_decrypt_failed: "auth",
  task_credentials_invalid: "auth",
  // Server-side authentication rejections belong with credential repair:
  // rotating the stored secret is the fix, not a blind retry. These codes
  // come from the HTTP family and the FTP/SFTP login paths.
  http_denied: "auth",
  ftp_auth_failed: "auth",
  sftp_auth_failed: "auth",
  // Tooling gaps belong with protocol handling, not "internal".
  ffmpeg_missing: "protocol",
  disk_write_failed: "disk",
  // A name clash at the save path is fixed by the same moves as a full disk
  // (another name or folder). Its backend category is "other", which would
  // file it under the catch-all now that the Attention Center groups by
  // concern too.
  final_path_conflict: "disk",
  // The restart-class codes name their concern outright. Without these a task
  // with no failure category fell through to "other", so the same task sat
  // under "Source" in the Attention Center and the catch-all here.
  remote_changed: "remoteChanged",
  resume_unavailable: "resume",
  resume_mismatch: "resume",
  temp_file_missing: "resume",
  temp_file_smaller_than_progress: "resume",
};

export function errorCodeForTask(task: Task): string | null {
  return task.errorCode ?? parseAppError(task.errorMessage)?.code ?? null;
}

export function recoveryConcern(task: Task): RecoveryConcern {
  const code = errorCodeForTask(task);
  if (code && code in CONCERN_BY_CODE) return CONCERN_BY_CODE[code];

  if (task.failureCategory && task.failureCategory in CONCERN_BY_CATEGORY) {
    return CONCERN_BY_CATEGORY[task.failureCategory];
  }

  // Fall back to the recovery actions the error carries, mirroring the
  // Attention Center's two-stage classification.
  const actions = task.recoveryActions.length > 0 ? task.recoveryActions : recoveryActionsForError(task.errorMessage);
  if (actions.includes("manage_sftp_host_keys")) return "auth";
  if (actions.includes("free_disk_space") || actions.includes("choose_another_folder")) return "disk";
  if (actions.includes("configure_ffmpeg")) return "protocol";
  if (actions.includes("check_url")) return "remoteChanged";
  return "other";
}

/**
 * Mirrors the backend bulk gate (commands/recovery.rs `bulk_resolution_gate`):
 * a task can be bulk-retried when it sits in a failure state and does not
 * require the destructive restart playbook.
 */
export function isAutoRecoverable(task: Task): boolean {
  if (task.status !== "failed" && task.status !== "needs_attention") return false;
  const code = errorCodeForTask(task);
  if (code === null) return true;
  if (RESTART_REQUIRED_CODES.has(code)) return false;
  // Publish never clobbers or auto-renames, so a path conflict persists until
  // the user acts — bulk-retrying it just re-fails (backend gate parity).
  return code !== "final_path_conflict";
}

export interface ConcernGroup {
  concern: RecoveryConcern;
  tasks: Task[];
  autoRecoverable: number;
  needsInput: number;
}

/**
 * Group tasks by concern. Groups with auto-retryable tasks come first, then
 * by group size, so the page leads with what one click can fix.
 */
export function groupRecoveryConcerns(tasks: Task[]): ConcernGroup[] {
  const grouped = new Map<RecoveryConcern, Task[]>();
  for (const concern of RECOVERY_CONCERNS) grouped.set(concern, []);
  for (const task of tasks) grouped.get(recoveryConcern(task))?.push(task);
  return RECOVERY_CONCERNS.map((concern) => {
    const members = grouped.get(concern) ?? [];
    const auto = members.filter(isAutoRecoverable).length;
    return {
      concern,
      tasks: members,
      autoRecoverable: auto,
      needsInput: members.length - auto,
    };
  })
    .filter((group) => group.tasks.length > 0)
    .sort((a, b) => b.autoRecoverable - a.autoRecoverable || b.tasks.length - a.tasks.length);
}

export interface PlaybookEntry {
  action: RecoveryAction;
  /** Lower rank = offered earlier (safer first, destructive last). */
  rank: number;
  /** What the action keeps on disk / in state (`recoveryCenter.playbook.*`). */
  keepsKey: TranslationKey;
  /** What the action deletes, if anything. */
  deletesKey: TranslationKey;
  redownloads: boolean;
  changesPath: boolean;
}

// §2.3 playbook: every action must state what it keeps, what it deletes,
// whether bytes are re-downloaded, and whether the final path can change.
// The table is exhaustive over RecoveryAction so a new backend action fails
// to compile until its consequences are documented.
export const PLAYBOOK: Record<RecoveryAction, Omit<PlaybookEntry, "action">> = {
  retry: {
    rank: 1,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  retry_later: {
    rank: 2,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  check_url: {
    rank: 3,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  free_disk_space: {
    rank: 4,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  manage_sftp_host_keys: {
    rank: 5,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  configure_ffmpeg: {
    rank: 6,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  open_folder: {
    rank: 7,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: false,
  },
  choose_another_name: {
    rank: 8,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: true,
  },
  choose_another_folder: {
    rank: 9,
    keepsKey: "recoveryCenter.playbook.retry.keeps",
    deletesKey: "recoveryCenter.playbook.nothing",
    redownloads: false,
    changesPath: true,
  },
  restart: {
    rank: 10,
    keepsKey: "recoveryCenter.playbook.restart.keeps",
    deletesKey: "recoveryCenter.playbook.restart.deletes",
    redownloads: true,
    changesPath: false,
  },
};

/** Actions that mutate task state (the ones recorded in recovery history). */
export const HISTORY_ACTION_KEYS: Record<string, TranslationKey> = {
  retry: "recoveryCenter.history.action.retry",
  retry_later: "recoveryCenter.history.action.retry_later",
  choose_another_name: "recoveryCenter.history.action.choose_another_name",
  choose_another_folder: "recoveryCenter.history.action.choose_another_folder",
  restart: "recoveryCenter.history.action.restart",
  update_credentials: "recoveryCenter.history.action.update_credentials",
};

/** Sources recorded by the backend for a recovery history row. */
export const HISTORY_SOURCE_KEYS: Record<string, TranslationKey> = {
  recovery_center: "recoveryCenter.history.source.recovery_center",
  manual: "recoveryCenter.history.source.manual",
  auto: "recoveryCenter.history.source.auto",
};

/** The task's recommended actions, safest first. */
export function playbookForTask(task: Task): PlaybookEntry[] {
  const actions = task.recoveryActions.length > 0 ? task.recoveryActions : recoveryActionsForError(task.errorMessage);
  return actions
    .filter((action) => action in PLAYBOOK)
    .map((action) => ({ action, ...PLAYBOOK[action] }))
    .sort((a, b) => a.rank - b.rank);
}
