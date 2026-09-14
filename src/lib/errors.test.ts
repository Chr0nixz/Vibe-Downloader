import { describe, expect, it } from "vitest";

import { SUPPORTED_LOCALES } from "@/i18n";
import en from "@/i18n/locales/en";
import es from "@/i18n/locales/es";
import ja from "@/i18n/locales/ja";
import ko from "@/i18n/locales/ko";
import ru from "@/i18n/locales/ru";
import zhCN from "@/i18n/locales/zh-CN";
import zhTW from "@/i18n/locales/zh-TW";
import {
  ERROR_CAUSE_I18N_MAP,
  errorCodeToI18nKey,
  errorMessage,
  isRecoveryAction,
  localizedErrorCause,
  localizedErrorMessage,
  localizedMessage,
  parseAppError,
  recoveryActionsForError,
} from "./errors";
import { ERROR_CODE_I18N_MAP, STABLE_ERROR_CODES, STABLE_ERROR_MESSAGES_EN } from "./stable-error-codes";

const LOCALE_BUNDLES: Record<string, unknown> = {
  en,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  ja,
  ko,
  ru,
  es,
};

function resolveKey(bundle: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>((node, part) => {
    if (node && typeof node === "object" && part in (node as Record<string, unknown>)) {
      return (node as Record<string, unknown>)[part];
    }
    return undefined;
  }, bundle);
}

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

describe("error cause copy", () => {
  it("maps every cause code to a stable code and to existing copy in every locale", () => {
    // The code→key table is invisible to check:i18n's literal scan, so this
    // walk is the guard that keeps every locale's cause copy present.
    const stableCodes: readonly string[] = STABLE_ERROR_CODES;
    for (const locale of SUPPORTED_LOCALES) {
      const bundle = LOCALE_BUNDLES[locale];
      expect(bundle, `locale bundle for ${locale}`).toBeTruthy();
      for (const [code, key] of Object.entries(ERROR_CAUSE_I18N_MAP)) {
        expect(stableCodes, `${code} must be a stable code`).toContain(code);
        const value = resolveKey(bundle, key);
        expect(typeof value === "string" && value.length > 0, `${key} missing in ${locale}`).toBe(true);
      }
    }
  });

  it("resolves causes for structured errors and stays silent otherwise", () => {
    const t = ((key: string) => `localized:${key}`) as never;
    const structured = JSON.stringify({
      code: "resume_unavailable",
      message: "Resume unavailable",
      recoverable: true,
      actions: ["restart"],
    });
    expect(localizedErrorCause(structured, t)).toBe("localized:errors.cause.resumeUnavailable");

    const unmapped = JSON.stringify({
      code: "dns_failure",
      message: "DNS lookup failed",
      recoverable: true,
      actions: [],
    });
    expect(localizedErrorCause(unmapped, t)).toBeUndefined();
    expect(localizedErrorCause("plain string failure", t)).toBeUndefined();
  });
});
