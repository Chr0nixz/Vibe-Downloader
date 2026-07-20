import type { TFunction } from "i18next";
import type { RecoveryAction } from "@/generated/bindings";
import { ERROR_CODE_I18N_MAP } from "@/lib/stable-error-codes";

export interface AppErrorPayload {
  code: string;
  message: string;
  recoverable: boolean;
  actions: string[];
}

const SUPPORTED_RECOVERY_ACTIONS = new Set<RecoveryAction>([
  "retry",
  "retry_later",
  "choose_another_name",
  "choose_another_folder",
  "restart",
  "open_folder",
  "check_url",
  "free_disk_space",
  "configure_ffmpeg",
  "manage_sftp_host_keys",
]);

export function parseAppError(error: unknown): AppErrorPayload | null {
  if (isAppErrorPayload(error)) return error;
  if (typeof error !== "string") return null;

  try {
    const parsed = JSON.parse(error) as unknown;
    return isAppErrorPayload(parsed) ? parsed : null;
  } catch {
    return legacyAppError(error);
  }
}

export function errorMessage(error: unknown): string {
  const payload = parseAppError(error);
  if (payload) return payload.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function errorCodeToI18nKey(code: string): string | null {
  return ERROR_CODE_I18N_MAP[code] ?? null;
}

export function localizedMessage(message: string | null | undefined, t: TFunction): string | undefined {
  if (!message) return undefined;
  if (message.startsWith("taskDiagnostics.")) {
    return t(message);
  }
  return message;
}

export function localizedErrorMessage(error: unknown, t: TFunction): string {
  const payload = parseAppError(error);
  if (payload) {
    if (payload.message.startsWith("taskDiagnostics.")) {
      return localizedMessage(payload.message, t) ?? "";
    }
    const i18nKey = errorCodeToI18nKey(payload.code);
    if (i18nKey) return t(i18nKey);
    // UX-11: never surface raw English backend messages for structured codes.
    return t("errors.unknownError");
  }
  return localizedMessage(errorMessage(error), t) ?? "";
}

/**
 * Format an error (structured payload, plain string, or unknown) into a
 * multi-line plain-text report suitable for copying to the clipboard when
 * the user wants to share diagnostics. Optional context (task id, url) is
 * included when available so the report is actionable for developers.
 */
export function formatErrorForReport(
  error: unknown,
  t: TFunction,
  context?: { taskId?: string; url?: string },
): string {
  const lines: string[] = [];
  if (context?.taskId) lines.push(`${t("errors.report.taskId")}: ${context.taskId}`);
  if (context?.url) lines.push(`${t("errors.report.url")}: ${context.url}`);
  const payload = parseAppError(error);
  if (payload) {
    lines.push(`${t("errors.report.code")}: ${payload.code}`);
    lines.push(`${t("errors.report.message")}: ${payload.message}`);
    lines.push(`${t("errors.report.recoverable")}: ${payload.recoverable ? "true" : "false"}`);
    if (payload.actions.length > 0) {
      lines.push(`${t("errors.report.actions")}: ${payload.actions.join(", ")}`);
    }
  } else {
    const message = errorMessage(error);
    lines.push(`${t("errors.report.message")}: ${message}`);
  }
  return lines.join("\n");
}

export function recoveryActionsForError(error: unknown): RecoveryAction[] {
  const payload = parseAppError(error);
  if (!payload) return [];
  const explicitActions = payload.actions.filter(isRecoveryAction);
  if (explicitActions.length > 0) return explicitActions;
  return fallbackActionsForCode(payload.code);
}

export function isRecoveryAction(action: string): action is RecoveryAction {
  return SUPPORTED_RECOVERY_ACTIONS.has(action as RecoveryAction);
}

function isAppErrorPayload(value: unknown): value is AppErrorPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<AppErrorPayload>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.recoverable === "boolean" &&
    Array.isArray(candidate.actions) &&
    candidate.actions.every((action) => typeof action === "string")
  );
}

function fallbackActionsForCode(code: string): RecoveryAction[] {
  switch (code) {
    case "duplicate_task":
      return [];
    case "final_path_conflict":
      return ["choose_another_name", "choose_another_folder", "retry"];
    case "remote_changed":
    case "resume_unavailable":
    case "resume_mismatch":
    case "temp_file_missing":
    case "temp_file_smaller_than_progress":
      return ["restart", "open_folder"];
    case "disk_write_failed":
      return ["free_disk_space", "choose_another_folder", "retry"];
    case "auth_headers_expired":
    case "auth_headers_unavailable":
      return ["check_url", "restart"];
    case "http_denied":
    case "http_not_found":
      return ["check_url", "retry"];
    case "server_rate_limited":
      return ["retry_later"];
    case "hls_ffmpeg_missing":
    case "dash_ffmpeg_missing":
    case "ffmpeg_missing":
    case "ffmpeg_not_found":
      return ["configure_ffmpeg"];
    case "sftp_host_key_changed":
      return ["manage_sftp_host_keys", "retry"];
    default:
      return [];
  }
}

function legacyAppError(error: string): AppErrorPayload | null {
  if (error.includes("Remote file changed")) {
    return {
      code: "remote_changed",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("remote_changed"),
    };
  }
  if (error.includes("Server no longer supports resume") || error.includes("Resume unavailable")) {
    return {
      code: "resume_unavailable",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("resume_unavailable"),
    };
  }
  if (error.includes("Temporary file is missing")) {
    return {
      code: "temp_file_missing",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("temp_file_missing"),
    };
  }
  if (error.includes("Temporary file is smaller")) {
    return {
      code: "temp_file_smaller_than_progress",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("temp_file_smaller_than_progress"),
    };
  }
  if (error.includes("disk") || error.includes("Disk") || error.includes("write") || error.includes("Write")) {
    return {
      code: "disk_write_failed",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("disk_write_failed"),
    };
  }
  if (/\b403\b/.test(error)) {
    return {
      code: "http_denied",
      message: error,
      recoverable: false,
      actions: fallbackActionsForCode("http_denied"),
    };
  }
  if (/\b404\b/.test(error)) {
    return {
      code: "http_not_found",
      message: error,
      recoverable: false,
      actions: fallbackActionsForCode("http_not_found"),
    };
  }
  if (/\b429\b/.test(error)) {
    return {
      code: "server_rate_limited",
      message: error,
      recoverable: true,
      actions: fallbackActionsForCode("server_rate_limited"),
    };
  }
  return null;
}
