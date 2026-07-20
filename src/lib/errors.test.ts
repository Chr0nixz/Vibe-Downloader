import { describe, expect, it } from "vitest";

import {
  errorCodeToI18nKey,
  errorMessage,
  isRecoveryAction,
  localizedErrorMessage,
  localizedMessage,
  parseAppError,
  recoveryActionsForError,
} from "./errors";
import { ERROR_CODE_I18N_MAP, STABLE_ERROR_CODES, STABLE_ERROR_MESSAGES_EN } from "./stable-error-codes";

describe("app error helpers", () => {
  it("parses structured app errors and filters supported actions", () => {
    const encoded = JSON.stringify({
      code: "server_rate_limited",
      message: "Retry after the server cool-down window.",
      recoverable: true,
      actions: ["retry_later", "unsupported_action"],
    });

    expect(parseAppError(encoded)).toMatchObject({
      code: "server_rate_limited",
      message: "Retry after the server cool-down window.",
      recoverable: true,
    });
    expect(errorMessage(encoded)).toBe("Retry after the server cool-down window.");
    expect(recoveryActionsForError(encoded)).toEqual(["retry_later"]);
  });

  it("falls back to code-based recovery actions when none are explicit", () => {
    const encoded = JSON.stringify({
      code: "disk_write_failed",
      message: "Could not write to disk.",
      recoverable: true,
      actions: [],
    });

    expect(recoveryActionsForError(encoded)).toEqual(["free_disk_space", "choose_another_folder", "retry"]);
  });

  it("maps legacy string errors into the structured recovery model", () => {
    const legacy = "HTTP 404 while requesting the file";

    expect(parseAppError(legacy)).toMatchObject({
      code: "http_not_found",
      recoverable: false,
    });
    expect(recoveryActionsForError(legacy)).toEqual(["check_url", "retry"]);
  });

  it("recognizes the supported recovery action surface", () => {
    expect(isRecoveryAction("restart")).toBe(true);
    expect(isRecoveryAction("configure_ffmpeg")).toBe(true);
    expect(isRecoveryAction("manage_sftp_host_keys")).toBe(true);
    expect(isRecoveryAction("delete_everything")).toBe(false);
  });

  it("localizes task diagnostic message keys without changing plain errors", () => {
    const t = ((key: string) => `localized:${key}`) as never;
    const encoded = JSON.stringify({
      code: "resume_unavailable",
      message: "taskDiagnostics.resumeUnavailable",
      recoverable: true,
      actions: ["restart"],
    });

    expect(localizedMessage("taskDiagnostics.completed", t)).toBe("localized:taskDiagnostics.completed");
    expect(localizedMessage("HTTP 404", t)).toBe("HTTP 404");
    expect(localizedErrorMessage(encoded, t)).toBe("localized:taskDiagnostics.resumeUnavailable");
  });

  it("maps every stable error code to an i18n key and never falls back to backend English", () => {
    const t = ((key: string) => `localized:${key}`) as never;
    expect(STABLE_ERROR_CODES.length).toBeGreaterThan(50);
    for (const code of STABLE_ERROR_CODES) {
      const i18nKey = errorCodeToI18nKey(code);
      expect(i18nKey).toBe(ERROR_CODE_I18N_MAP[code]);
      expect(i18nKey).toMatch(/^errors\./);
      expect(STABLE_ERROR_MESSAGES_EN[code]).toBeTruthy();

      const encoded = JSON.stringify({
        code,
        message: `BACKEND ENGLISH FOR ${code}`,
        recoverable: true,
        actions: [],
      });
      const localized = localizedErrorMessage(encoded, t);
      expect(localized).toBe(`localized:${i18nKey}`);
      expect(localized).not.toContain("BACKEND ENGLISH");
    }
  });

  it("uses the unknownError key for structured codes missing from the map", () => {
    const t = ((key: string) => `localized:${key}`) as never;
    const encoded = JSON.stringify({
      code: "totally_unknown_future_code",
      message: "Raw English backend message",
      recoverable: false,
      actions: [],
    });
    expect(localizedErrorMessage(encoded, t)).toBe("localized:errors.unknownError");
  });
});
