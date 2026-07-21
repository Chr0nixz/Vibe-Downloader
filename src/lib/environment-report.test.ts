import { describe, expect, it } from "vitest";

import type { EnvironmentHealthReport } from "@/generated/bindings";
import { formatEnvironmentReport } from "@/lib/environment-report";

const sampleReport: EnvironmentHealthReport = {
  checkedAtMs: "1700000000000",
  appVersion: "0.3.0",
  platform: "windows-x86_64",
  items: [
    {
      id: "ffmpeg",
      status: "error",
      summary: "ffmpeg was not found.",
      detail: "password=secret123 path=C:\\tools",
      suggestedActions: [],
    },
    {
      id: "proxy",
      status: "ok",
      summary: "Proxy is disabled.",
      detail: null,
      suggestedActions: [],
    },
  ],
};

describe("formatEnvironmentReport", () => {
  it("includes version, platform, items, and updater without leaking secrets", () => {
    const text = formatEnvironmentReport(sampleReport, {
      currentVersion: "0.3.0",
      updateVersion: null,
      status: "up-to-date",
      error: null,
    });
    expect(text).toContain("App version: 0.3.0");
    expect(text).toContain("Platform: windows-x86_64");
    expect(text).toContain("[ERROR] ffmpeg:");
    expect(text).toContain("[OK] proxy:");
    expect(text).toContain("status: up-to-date");
    expect(text).not.toContain("secret123");
    expect(text).toContain("password=[redacted]");
  });
});
