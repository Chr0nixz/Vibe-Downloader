import { beforeAll, describe, expect, it, vi } from "vitest";

async function loadSubject() {
  return import("./use-task-events");
}

beforeAll(() => {
  vi.stubGlobal("localStorage", {
    getItem: () => null,
    setItem: () => undefined,
    removeItem: () => undefined,
  });
  vi.stubGlobal("navigator", { language: "en-US" });
  vi.stubGlobal("document", { documentElement: { lang: "" } });
});

describe("task event helpers", () => {
  it("builds a localized desktop status with aggregate progress and error state", async () => {
    const { buildDesktopStatusUpdate } = await loadSubject();
    const update = buildDesktopStatusUpdate({
      all: "4",
      active: "2",
      queued: "1",
      attention: "1",
      paused: "0",
      waitingNetwork: "0",
      completed: "0",
      failed: "0",
      totalSpeed: "2048",
      totalDownloaded: "25",
      totalBytes: "100",
      featuredTaskId: null,
    });

    expect(update.progress).toBe(25);
    expect(update.hasError).toBe(true);
    expect(update.tooltip).toContain("2");
    expect(update.tooltip).toContain("1");
  });

  it("omits taskbar progress when no active task has a known total", async () => {
    const { buildDesktopStatusUpdate } = await loadSubject();
    const update = buildDesktopStatusUpdate({
      all: "1",
      active: "0",
      queued: "1",
      attention: "0",
      paused: "0",
      waitingNetwork: "0",
      completed: "0",
      failed: "0",
      totalSpeed: "0",
      totalDownloaded: "0",
      totalBytes: "0",
      featuredTaskId: null,
    });

    expect(update.progress).toBeNull();
    expect(update.hasError).toBe(false);
  });

  it("aggregates failure counts across the notification debounce window", async () => {
    const { mergeFailureNotificationCounts } = await loadSubject();
    const first = mergeFailureNotificationCounts({ failed: 0, attention: 0 }, 2, 1);
    expect(mergeFailureNotificationCounts(first, 1, 3)).toEqual({ failed: 3, attention: 4 });
  });

  it("caps remembered status notifications and preserves recent keys", async () => {
    const { rememberStatusNotification } = await loadSubject();
    const statuses = new Set(["task-1:completed", "task-2:failed"]);

    expect(rememberStatusNotification(statuses, "task-3:completed", 2)).toBe(true);
    expect([...statuses]).toEqual(["task-2:failed", "task-3:completed"]);
  });

  it("does not remember the same status notification twice", async () => {
    const { rememberStatusNotification } = await loadSubject();
    const statuses = new Set(["task-1:completed"]);

    expect(rememberStatusNotification(statuses, "task-1:completed", 2)).toBe(false);
    expect([...statuses]).toEqual(["task-1:completed"]);
  });
});
