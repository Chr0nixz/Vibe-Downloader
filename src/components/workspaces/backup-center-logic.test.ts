import { describe, expect, it } from "vitest";

import {
  bytesToNumber,
  CONTENTS_FIELD_KEYS,
  CONTENTS_FIELD_ORDER,
  diskCheckLevel,
  NOT_EXPORTED_ITEMS,
  NOT_EXPORTED_KEYS,
  reportEntries,
  SUBSET_FIELD_KEYS,
  SUBSET_HINT_KEYS,
} from "@/components/workspaces/backup-center-logic";

/**
 * The inventory/not-exported/subset/report keys are resolved from tables
 * rather than literal `t("...")` calls, so `pnpm check:i18n` cannot see them.
 * This walk keeps the tables and all locales in step (same guard as
 * recovery-center-logic).
 */
describe("backup center key tables", () => {
  it("resolves every table key in every locale", async () => {
    const i18n = (await import("@/i18n")).default;
    const { SUPPORTED_LOCALES } = await import("@/i18n");
    const previous = i18n.language;
    try {
      for (const locale of SUPPORTED_LOCALES) {
        await i18n.changeLanguage(locale);
        for (const item of NOT_EXPORTED_ITEMS) {
          expect(i18n.exists(NOT_EXPORTED_KEYS[item]), `${locale} missing ${item}`).toBe(true);
        }
        for (const field of CONTENTS_FIELD_ORDER) {
          expect(i18n.exists(CONTENTS_FIELD_KEYS[field]), `${locale} missing ${field}`).toBe(true);
        }
        for (const field of Object.keys(SUBSET_FIELD_KEYS) as Array<keyof typeof SUBSET_FIELD_KEYS>) {
          expect(i18n.exists(SUBSET_FIELD_KEYS[field]), `${locale} missing subset ${field}`).toBe(true);
          expect(i18n.exists(SUBSET_HINT_KEYS[field]), `${locale} missing subset hint ${field}`).toBe(true);
        }
        for (const key of [
          "backupCenter.title",
          "backupCenter.subtitle",
          "backupCenter.loading",
          "backupCenter.pickFileTitle",
          "backupCenter.pickFile",
          "backupCenter.exportTitle",
          "backupCenter.exportButton",
          "backupCenter.exportResultPath",
          "backupCenter.usedCopyFallback",
          "backupCenter.notExportedTitle",
          "backupCenter.restoreTitle",
          "backupCenter.manifest.appVersion",
          "backupCenter.manifest.schemaVersion",
          "backupCenter.manifest.databaseBytes",
          "backupCenter.check.disk",
          "backupCenter.check.diskUnknownValue",
          "backupCenter.check.pathsOk",
          "backupCenter.check.pathsWarning",
          "backupCenter.check.pathsWarning_one",
          "backupCenter.check.offendingDirs",
          "backupCenter.check.scrubFfmpeg",
          "backupCenter.check.scrubCompletion",
          "backupCenter.check.scrubProxy",
          "backupCenter.check.defaultSaveDir",
          "backupCenter.remap.toggle",
          "backupCenter.remap.description",
          "backupCenter.remap.pick",
          "backupCenter.remap.pickTitle",
          "backupCenter.remap.remappedPaths",
          "backupCenter.remap.remappedPaths_one",
          "backupCenter.confirm.open",
          "backupCenter.confirm.title",
          "backupCenter.confirm.description",
          "backupCenter.confirm.warning",
          "backupCenter.confirm.confirm",
          "backupCenter.confirm.cancel",
          "backupCenter.subsetTitle",
          "backupCenter.subsetDescription",
          "backupCenter.subset.run",
          "backupCenter.subset.resultSummary",
          "backupCenter.reportTitle",
          "backupCenter.report.created",
          "backupCenter.report.dismiss",
          "backupCenter.report.credentials",
          "backupCenter.report.credentials_one",
          "backupCenter.report.proxyGlobal",
          "backupCenter.report.proxy",
          "backupCenter.report.proxy_one",
          "backupCenter.report.ffmpeg",
          "backupCenter.report.completion",
          "backupCenter.report.missingDirs",
          "backupCenter.report.rollback",
          "backupCenter.toast.loadFailed",
          "backupCenter.toast.exportSuccess",
          "backupCenter.toast.exportFailed",
          "backupCenter.toast.validateFailed",
          "backupCenter.toast.restoreSuccess",
          "backupCenter.toast.restoreFailed",
          "backupCenter.toast.subsetSuccess",
          "backupCenter.toast.subsetFailed",
          "backupCenter.toast.dismissFailed",
          "nav.backup",
        ] as const) {
          expect(i18n.exists(key), `${locale} missing ${key}`).toBe(true);
        }
      }
    } finally {
      await i18n.changeLanguage(previous);
    }
  });
});

describe("reportEntries", () => {
  it("orders entries by impact and skips absent conditions", () => {
    const entries = reportEntries({
      tasksWithCredentials: 3,
      tasksWithPerTaskProxy: 2,
      globalProxyNeedsReentry: true,
      ffmpegWasConfigured: true,
      completionActionReset: true,
      missingSaveDirs: ["/old/a", "/old/b"],
      missingSaveDirsTotal: 5,
      preRestoreBackupPath: "/bak/vibe.db.bak-1",
      backupCreatedAt: null,
    });
    expect(entries.map((entry) => entry.key)).toEqual([
      "backupCenter.report.credentials",
      "backupCenter.report.proxyGlobal",
      "backupCenter.report.proxy",
      "backupCenter.report.ffmpeg",
      "backupCenter.report.completion",
      "backupCenter.report.missingDirs",
      "backupCenter.report.rollback",
    ]);
    expect(entries[0].values).toEqual({ count: 3 });
    // 2 shown + 3 more → the +N suffix keeps the banner bounded.
    expect(entries[5].values).toEqual({ dirs: "/old/a, /old/b +3" });
  });

  it("produces nothing for a clean same-machine restore", () => {
    const entries = reportEntries({
      tasksWithCredentials: 0,
      tasksWithPerTaskProxy: 0,
      globalProxyNeedsReentry: false,
      ffmpegWasConfigured: false,
      completionActionReset: false,
      missingSaveDirs: [],
      missingSaveDirsTotal: 0,
      preRestoreBackupPath: null,
      backupCreatedAt: null,
    });
    expect(entries).toEqual([]);
  });
});

describe("diskCheckLevel", () => {
  it("compares the decimal-string byte counts", () => {
    expect(diskCheckLevel("1000", "999")).toBe("ok");
    expect(diskCheckLevel("5", "6")).toBe("low");
    expect(diskCheckLevel(null, "6")).toBe("unknown");
  });
});

describe("bytesToNumber", () => {
  it("parses decimal strings and degrades garbage to zero", () => {
    expect(bytesToNumber("123456")).toBe(123456);
    expect(bytesToNumber("")).toBe(0);
    expect(bytesToNumber("not-a-number")).toBe(0);
  });
});
