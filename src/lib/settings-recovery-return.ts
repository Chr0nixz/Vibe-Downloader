import type { RecoveryAction } from "@/generated/bindings";

export type SettingsRecoveryFocus = "sftp_known_hosts" | "ffmpeg_path";

export type SettingsRecoveryReturn = {
  focus: SettingsRecoveryFocus;
  taskId: string;
  action: Extract<RecoveryAction, "configure_ffmpeg" | "manage_sftp_host_keys">;
};

const FOCUS_KEY = "vibe-settings-focus";
const RETURN_TASK_KEY = "vibe-settings-return-task";
const RETURN_ACTION_KEY = "vibe-settings-return-action";

function write(key: string, value: string): void {
  try {
    sessionStorage.setItem(key, value);
  } catch {
    // sessionStorage may be unavailable in locked-down environments.
  }
}

function read(key: string): string | null {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function remove(key: string): void {
  try {
    sessionStorage.removeItem(key);
  } catch {
    // Ignore storage failures.
  }
}

export function writeSettingsRecoveryReturn(input: SettingsRecoveryReturn): void {
  write(FOCUS_KEY, input.focus);
  write(RETURN_TASK_KEY, input.taskId);
  write(RETURN_ACTION_KEY, input.action);
}

export function readSettingsRecoveryReturn(): SettingsRecoveryReturn | null {
  const taskId = read(RETURN_TASK_KEY);
  const action = read(RETURN_ACTION_KEY);
  if (!taskId) return null;
  if (action !== "configure_ffmpeg" && action !== "manage_sftp_host_keys") return null;
  // Focus may already have been consumed to scroll the matching settings row.
  const focus: SettingsRecoveryFocus = action === "configure_ffmpeg" ? "ffmpeg_path" : "sftp_known_hosts";
  return { focus, taskId, action };
}

export function clearSettingsRecoveryReturn(): void {
  remove(FOCUS_KEY);
  remove(RETURN_TASK_KEY);
  remove(RETURN_ACTION_KEY);
}

export function consumeSettingsFocus(): SettingsRecoveryFocus | null {
  const focus = read(FOCUS_KEY);
  if (focus !== "sftp_known_hosts" && focus !== "ffmpeg_path") return null;
  remove(FOCUS_KEY);
  return focus;
}
