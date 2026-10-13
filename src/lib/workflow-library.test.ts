import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BulkTaskOptionsPatch, WorkflowEntry } from "@/generated/bindings";
import {
  applySiteProfile,
  deleteWorkflowEntry,
  listWorkflowLibrary,
  publicSiteProfile,
  runBulkOptions,
  safePreset,
  saveWorkflowEntry,
  siteOrigin,
} from "./workflow-library";

vi.mock("@/lib/runtime", () => ({ isTauriRuntime: () => false }));
const patch: BulkTaskOptionsPatch = {
  priority: "low",
  taskSpeedLimitBps: null,
  clearSpeedLimit: false,
  categoryKey: null,
  clearCategory: false,
  obeySchedule: null,
};
beforeEach(() => localStorage.clear());
describe("workflow library", () => {
  it("reuses public headers for the exact origin and keeps task-only secrets", () => {
    const profile = publicSiteProfile("https://example.com/file?token=URL_SECRET", {
      userAgent: "Downloader",
      referer: "https://example.com/download?token=REFERER_SECRET",
      customHeaders: "Cookie: COOKIE_SECRET\nX-Api-Key: API_SECRET\nAccept: application/octet-stream",
    });
    expect(JSON.stringify(profile)).not.toContain("SECRET");
    expect(profile.origin).toBe("https://example.com");
    const result = applySiteProfile(
      "https://example.com/next",
      { userAgent: "Old", referer: "", customHeaders: "Cookie: TASK_SECRET\nAccept: old" },
      profile,
    );
    expect(result.customHeaders).toBe("accept: application/octet-stream\ncookie: TASK_SECRET");
    expect(() => applySiteProfile("http://example.com/next", result, profile)).toThrow();
    expect(() => applySiteProfile("https://example.com:8443/next", result, profile)).toThrow();
    expect(siteOrigin("https://user:secret@example.com/file")).toBeNull();
  });
  it("strips proxy credentials from presets and keeps off/inherit semantics", () => {
    const preset = safePreset({
      saveDir: "D:/Downloads",
      categoryKey: null,
      priority: "high",
      taskSpeedLimitBps: "2048",
      obeySchedule: false,
      proxyMode: "custom",
      proxyUrl: "socks5://user:SECRET@localhost:1080",
      proxyNoProxy: "localhost",
    });
    expect(preset.proxyUrl).toBe("socks5://localhost:1080");
    expect(JSON.stringify(preset)).not.toContain("SECRET");
    expect(safePreset({ ...preset, proxyMode: "off" }).proxyUrl).toBeNull();
  });
  it("persists named views, supports rename/copy/delete and preserves future versions", async () => {
    const entry: WorkflowEntry = {
      id: crypto.randomUUID(),
      name: "Failures",
      data: {
        kind: "saved_view",
        value: {
          nav: "issues",
          search: "model",
          sortKey: "updated_at",
          sortDirection: "desc",
          fileType: "all",
          source: "all",
          failure: "all",
          resume: "all",
        },
      },
    };
    await saveWorkflowEntry(entry);
    expect((await listWorkflowLibrary()).entries).toEqual([entry]);
    await saveWorkflowEntry({ ...entry, name: "Renamed" });
    await saveWorkflowEntry({ ...entry, id: crypto.randomUUID(), name: "Copy" });
    const rows = JSON.parse(localStorage.getItem("vibe-workflow-library-v1") ?? "[]");
    rows.push({ version: 2, entry: { id: "future" } });
    rows.push(null);
    localStorage.setItem("vibe-workflow-library-v1", JSON.stringify(rows));
    const secretEntry = { ...entry, password: "SECRET" };
    await expect(saveWorkflowEntry(secretEntry)).rejects.toThrow();
    await saveWorkflowEntry({ ...entry, name: "Retained" });
    await deleteWorkflowEntry(entry.id);
    const library = await listWorkflowLibrary();
    expect(library.entries.map((row) => row.name)).toEqual(["Copy"]);
    expect(library.unsupportedCount).toBe(2);
    expect(localStorage.getItem("vibe-workflow-library-v1")).toContain("future");
  });
  it("chunks explicit selections, retains partial results and stops before another chunk", async () => {
    let cancelled = false;
    const submit = vi.fn(async (ids: string[]) => {
      cancelled = true;
      return ids.map((id) => ({ id, status: "applied", error: null }));
    });
    const ids = Array.from({ length: 205 }, (_, index) => `task-${index}`);
    const results = await runBulkOptions([...ids, ids[0]], patch, {
      submit,
      isCancelled: () => cancelled,
      onProgress: () => {},
    });
    expect(submit).toHaveBeenCalledTimes(1);
    expect(results.filter((row) => row.status === "applied")).toHaveLength(100);
    expect(results.filter((row) => row.status === "not_processed")).toHaveLength(105);
  });
  it("records failed chunks without losing successful items", async () => {
    const submit = vi
      .fn()
      .mockRejectedValueOnce(new Error("IPC failed"))
      .mockImplementation(async (ids: string[]) => ids.map((id) => ({ id, status: "applied", error: null })));
    const results = await runBulkOptions(
      Array.from({ length: 101 }, (_, i) => String(i)),
      patch,
      { submit, isCancelled: () => false, onProgress: () => {} },
    );
    expect(results.filter((row) => row.status === "failed")).toHaveLength(100);
    expect(results[100]?.status).toBe("applied");
  });
});
