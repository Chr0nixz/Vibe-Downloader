import { describe, expect, it } from "vitest";

import {
  settingsSearchHasResults,
  settingsSectionMatchesQuery,
  type SettingsSearchSection,
} from "./settings-search";

const sections: SettingsSearchSection[] = [
  {
    id: "downloads",
    title: "Downloads",
    description: "Defaults for new tasks",
    summary: "2 active / Unlimited",
    terms: ["Default save directory", "Global speed limit"],
  },
  {
    id: "network",
    title: "Network",
    summary: "Direct connection",
    terms: ["Proxy URL", "Proxy password"],
  },
  {
    id: "browser-integration",
    title: "Browser integration",
    summary: "Chrome/Edge installed",
    terms: ["Native host", "Extension packages"],
  },
];

describe("settings search helpers", () => {
  it("matches terms inside collapsed settings sections", () => {
    expect(settingsSectionMatchesQuery(sections[1], "proxy password")).toBe(true);
    expect(settingsSectionMatchesQuery(sections[2], "chrome")).toBe(true);
  });

  it("reports an empty state when the query does not match any section context", () => {
    expect(settingsSearchHasResults(sections, "sftp account")).toBe(false);
    expect(settingsSearchHasResults(sections, "global speed")).toBe(true);
  });
});
