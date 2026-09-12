import { describe, expect, it } from "vitest";

import type { EnvironmentHealthReport, EnvironmentText } from "@/generated/bindings";
import i18n from "@/i18n";
import { formatEnvironmentReport } from "@/lib/environment-report";

const NO_PARAMS = {
  count: null,
  names: null,
  url: null,
  errors: null,
  path: null,
  section: null,
  available: null,
  total: null,
};

function text(code: EnvironmentText["code"], english: string): EnvironmentText {
  return { code, params: NO_PARAMS, english };
}

const sampleReport: EnvironmentHealthReport = {
  checkedAtMs: "1700000000000",
  appVersion: "0.3.0",
  platform: "windows-x86_64",
  items: [
    {
      id: "ffmpeg",
      status: "error",
      summary: text("ffmpegMissing", "ffmpeg was not found. HLS/DASH remuxing will fail."),
      detail: [text("raw", "password=secret123 path=C:\\tools")],
      suggestedActions: [],
    },
    {
      id: "proxy",
      status: "ok",
      summary: text("proxyDisabled", "Proxy is disabled."),
      detail: [],
      suggestedActions: [],
    },
  ],
};

const updater = {
  currentVersion: "0.3.0",
  updateVersion: null,
  status: "up-to-date" as const,
  error: null,
};

const report = () => formatEnvironmentReport(sampleReport, updater, i18n.t.bind(i18n));

describe("formatEnvironmentReport", () => {
  it("includes version, platform, items, and updater without leaking secrets", () => {
    const output = report();
    expect(output).toContain("App version: 0.3.0");
    expect(output).toContain("Platform: windows-x86_64");
    expect(output).toContain("[ERROR] ffmpeg:");
    expect(output).toContain("[OK] proxy:");
    expect(output).toContain("status: up-to-date");
    expect(output).not.toContain("secret123");
    expect(output).toContain("password=[redacted]");
  });

  it("renders item copy through the active locale, not the English source text", async () => {
    const previous = i18n.language;
    try {
      await i18n.changeLanguage("zh-CN");
      const output = report();
      expect(output).toContain("未找到 ffmpeg");
      expect(output).not.toContain("ffmpeg was not found.");
    } finally {
      await i18n.changeLanguage(previous);
    }
  });

  it("prints raw fragments verbatim", () => {
    expect(report()).toContain("path=C:\\tools");
  });
});
