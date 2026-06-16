import { describe, expect, it } from "vitest";

import {
  errorMessage,
  isRecoveryAction,
  localizedErrorMessage,
  localizedMessage,
  parseAppError,
  recoveryActionsForError,
} from "./errors";

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

    expect(recoveryActionsForError(encoded)).toEqual([
      "free_disk_space",
      "choose_another_folder",
      "retry",
    ]);
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

    expect(localizedMessage("taskDiagnostics.completed", t)).toBe(
      "localized:taskDiagnostics.completed",
    );
    expect(localizedMessage("HTTP 404", t)).toBe("HTTP 404");
    expect(localizedErrorMessage(encoded, t)).toBe(
      "localized:taskDiagnostics.resumeUnavailable",
    );
  });
});
