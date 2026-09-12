import { describe, expect, it } from "vitest";

import { ENVIRONMENT_TEXT_KEYS, environmentTextKey, formatEnvironmentText } from "@/lib/environment-text";

/**
 * FUN-28 guard: the backend can emit any `EnvironmentTextCode`, and the frontend
 * resolves it through a table rather than a literal `t("...")` call, so
 * `pnpm check:i18n` cannot see these keys. This test is the gate that keeps the
 * two sides in step.
 */
describe("environment text codes", () => {
  it("maps every non-raw code to a key that exists in every locale", async () => {
    const i18n = (await import("@/i18n")).default;
    const { SUPPORTED_LOCALES } = await import("@/i18n");
    const previous = i18n.language;

    try {
      for (const locale of SUPPORTED_LOCALES) {
        await i18n.changeLanguage(locale);
        for (const [code, key] of Object.entries(ENVIRONMENT_TEXT_KEYS)) {
          expect(i18n.exists(key), `${locale} is missing ${key} (code ${code})`).toBe(true);
        }
      }
    } finally {
      await i18n.changeLanguage(previous);
    }
  });

  it("has no key for raw, which is printed verbatim", () => {
    expect(environmentTextKey("raw")).toBeNull();
    expect(
      formatEnvironmentText(
        {
          code: "raw",
          params: {
            count: null,
            names: null,
            url: null,
            errors: null,
            path: null,
            section: null,
            available: null,
            total: null,
          },
          english: "C:\\tools\\ffmpeg.exe",
        },
        (() => "should not be called") as never,
      ),
    ).toBe("C:\\tools\\ffmpeg.exe");
  });

  it("falls back to the English source when the backend is newer than the bundle", () => {
    // A code this build has never heard of must degrade to the backend's own
    // text rather than rendering a bare key name.
    const unknown = { code: "someFutureCode", english: "Something new happened." } as never;
    expect(formatEnvironmentText(unknown, (() => "should not be called") as never)).toBe("Something new happened.");
  });
});
