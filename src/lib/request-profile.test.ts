import { describe, expect, it } from "vitest";
import { EMPTY_REQUEST_PROFILE, parseRequestProfile, requestProfileDraft } from "./request-profile";

describe("task request profiles", () => {
  it("normalizes headers and retains colons inside values", () => {
    expect(
      parseRequestProfile({
        userAgent: " SiteAgent/1 ",
        referer: "https://example.com/page",
        customHeaders: " X-Token: prefix:value\nAccept-Language: zh-CN",
      }),
    ).toEqual({
      userAgent: "SiteAgent/1",
      referer: "https://example.com/page",
      customHeaders: [
        { name: "x-token", value: "prefix:value" },
        { name: "accept-language", value: "zh-CN" },
      ],
    });
    expect(parseRequestProfile(EMPTY_REQUEST_PROFILE)).toBeNull();
  });
  it("rejects reserved names, duplicates, invalid values and size excesses without echoing secrets", () => {
    for (const customHeaders of [
      "Range: secret",
      "Authorization: secret",
      "User-Agent: secret",
      "X-Forwarded-For: secret",
      "Cookie: secret\ncOOkie: other",
      "No colon",
      "X-Token: ",
      "X-Token: secret\u0000",
      "\tCookie: secret",
      `${" ".repeat(129)}Cookie: secret`,
      Array.from({ length: 17 }, (_, index) => `X-${index}: a`).join("\n"),
    ]) {
      expect(() => parseRequestProfile({ ...EMPTY_REQUEST_PROFILE, customHeaders })).toThrow("request_profile_invalid");
    }
    expect(() => parseRequestProfile({ ...EMPTY_REQUEST_PROFILE, userAgent: "secret\n" })).toThrow();
    expect(() =>
      parseRequestProfile({ ...EMPTY_REQUEST_PROFILE, customHeaders: `X-Token: ${"x".repeat(8193)}` }),
    ).toThrow();
    for (const referer of ["file:///tmp/secret", "https://user:secret@example.com/", "https://example.com/#secret"]) {
      expect(() => parseRequestProfile({ ...EMPTY_REQUEST_PROFILE, referer })).toThrow();
    }
  });
  it("prefills ordinary fields without revealing sensitive values", () => {
    expect(
      requestProfileDraft({
        taskId: "task",
        userAgent: "Agent/1",
        referer: null,
        customHeaders: [{ name: "accept", value: "application/octet-stream" }],
        sensitiveHeaderNames: ["cookie", "x-token"],
        sensitiveExpiresAt: "2026-10-03T00:00:00Z",
        sensitiveExpired: false,
      }),
    ).toEqual({ userAgent: "Agent/1", referer: "", customHeaders: "accept: application/octet-stream" });
  });
});
