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
