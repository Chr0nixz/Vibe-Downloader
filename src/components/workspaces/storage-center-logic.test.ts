import { describe, expect, it } from "vitest";

import {
  groupReclaimable,
  groupResumableByTask,
  itemsForMode,
  parseBytes,
  STORAGE_KIND_KEYS,
  STORAGE_REASON_KEYS,
  STORAGE_SWEEP_MODE_KEYS,
  scanHasReclaimable,
  summarizeCleanup,
  totalsFor,
} from "@/components/workspaces/storage-center-logic";
import type { StorageArtifactItem, StorageCleanupResult } from "@/generated/bindings";

/**
 * The reason/kind/sweep-mode keys are resolved from backend code strings via
 * tables rather than literal `t("...")` calls, so `pnpm check:i18n` cannot see
 * them. This walk is the gate that keeps the tables and all locales in step
 * (same guard as environment-text).
 */
describe("storage center key tables", () => {
  it("maps every reason, kind, and sweep-mode key in every locale", async () => {
    const i18n = (await import("@/i18n")).default;
    const { SUPPORTED_LOCALES } = await import("@/i18n");
    const previous = i18n.language;
    try {
      for (const locale of SUPPORTED_LOCALES) {
        await i18n.changeLanguage(locale);
        for (const [code, key] of Object.entries(STORAGE_REASON_KEYS)) {
          expect(i18n.exists(key), `${locale} missing ${key} (reason ${code})`).toBe(true);
        }
        for (const [kind, key] of Object.entries(STORAGE_KIND_KEYS)) {
          expect(i18n.exists(key), `${locale} missing ${key} (kind ${kind})`).toBe(true);
        }
        for (const [mode, key] of Object.entries(STORAGE_SWEEP_MODE_KEYS)) {
          expect(i18n.exists(key), `${locale} missing ${key} (sweep mode ${mode})`).toBe(true);
        }
      }
    } finally {
      await i18n.changeLanguage(previous);
    }
  });
});

function makeItem(overrides: Partial<StorageArtifactItem> & { id: string }): StorageArtifactItem {
  return {
    kind: "temp_file",
    saveDir: "/downloads",
    fileName: overrides.id,
    bytes: "1000",
    modifiedAt: null,
    reclaimable: true,
    reason: "no_owner",
    ownerTaskId: null,
    taskFileName: null,
    ownerProtocol: null,
    ...overrides,
  };
}

describe("groupReclaimable", () => {
  it("groups by kind, largest total first, and ignores kept items", () => {
    const groups = groupReclaimable([
      makeItem({ id: "a", kind: "temp_file", bytes: "100" }),
      makeItem({ id: "b", kind: "staging_dir", bytes: "5000" }),
      makeItem({ id: "c", kind: "temp_file", bytes: "300" }),
      makeItem({ id: "kept", reclaimable: false, bytes: "9999" }),
    ]);
    expect(groups.map((group) => group.kind)).toEqual(["staging_dir", "temp_file"]);
    expect(groups[1].totalBytes).toBe(400);
    expect(groups[1].items.map((item) => item.id)).toEqual(["c", "a"]);
  });
});

describe("groupResumableByTask", () => {
  it("groups kept items by owner and skips reclaimable rows", () => {
    const groups = groupResumableByTask([
      makeItem({ id: "a", reclaimable: false, ownerTaskId: "t1", taskFileName: "one.bin", ownerProtocol: "https" }),
      makeItem({ id: "b", reclaimable: false, ownerTaskId: "t1", bytes: "700" }),
      makeItem({ id: "c", reclaimable: true, bytes: "5000" }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].taskId).toBe("t1");
    expect(groups[0].taskFileName).toBe("one.bin");
    expect(groups[0].totalBytes).toBe(1700);
  });
});

describe("totalsFor and itemsForMode", () => {
  const items = [
    makeItem({ id: "orphan", reason: "no_owner", bytes: "100" }),
    makeItem({ id: "completed", reason: "owner_completed", bytes: "200" }),
    makeItem({ id: "kept", reclaimable: false, bytes: "400" }),
  ];

  it("computes byte and count totals", () => {
    expect(totalsFor(items)).toEqual({
      reclaimableBytes: 300,
      reclaimableCount: 2,
      resumableBytes: 400,
    });
  });

  it("filters per cleanup mode", () => {
    expect(itemsForMode(items, "orphans").map((item) => item.id)).toEqual(["orphan"]);
    expect(itemsForMode(items, "completed_leftovers").map((item) => item.id)).toEqual(["completed"]);
    expect(itemsForMode(items, "all_reclaimable")).toHaveLength(2);
    expect(itemsForMode(items, "selected")).toHaveLength(2);
  });
});

describe("summarizeCleanup", () => {
  it("separates removed, skipped, and failed outcomes", () => {
    const result = {
      removedCount: 2,
      skippedCount: 1,
      failedCount: 1,
      outcomes: [
        { itemId: "a", outcome: "removed", bytes: "1", errorCode: null },
        { itemId: "b", outcome: "removed", bytes: "1", errorCode: null },
        { itemId: "c", outcome: "skipped", bytes: "0", errorCode: null },
        { itemId: "d", outcome: "failed", bytes: "0", errorCode: "storage_cleanup_failed" },
      ],
    } as unknown as StorageCleanupResult;
    const report = summarizeCleanup(result);
    expect(report.removedCount).toBe(2);
    expect(report.skippedCount).toBe(1);
    expect(report.failedCount).toBe(1);
    expect(report.failures).toEqual([{ itemId: "d", errorCode: "storage_cleanup_failed" }]);
  });
});

describe("scan helpers", () => {
  it("parseBytes rejects non-numeric strings", () => {
    expect(parseBytes("123")).toBe(123);
    expect(parseBytes("nope")).toBe(0);
  });

  it("scanHasReclaimable handles null and empty scans", () => {
    expect(scanHasReclaimable(null)).toBe(false);
    expect(
      scanHasReclaimable({
        scanId: "s",
        scannedAt: "",
        dirs: [],
        items: [makeItem({ id: "kept", reclaimable: false })],
      }),
    ).toBe(false);
  });
});
