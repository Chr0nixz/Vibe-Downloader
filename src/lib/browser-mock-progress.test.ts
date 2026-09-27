import { describe, expect, it } from "vitest";

import { advanceBrowserMockProgress } from "./browser-mock-progress";

describe("advanceBrowserMockProgress", () => {
  it("settles a persisted active task whose bytes already reached the total", () => {
    const progress = advanceBrowserMockProgress(
      {
        status: "downloading",
        totalSize: 100,
        downloadedBytes: 100,
        speedBps: 1_000,
        connectionCount: 4,
        healthSummary: "taskDiagnostics.downloading",
      },
      "2026-09-27T00:00:00.000Z",
    );

    expect(progress).toEqual({
      status: "completed",
      downloadedBytes: 100,
      speedBps: 0,
      connectionCount: 0,
      healthSummary: "taskDiagnostics.completed",
      updatedAt: "2026-09-27T00:00:00.000Z",
      completed: true,
    });
  });

  it("advances incomplete work without changing its active status", () => {
    const progress = advanceBrowserMockProgress(
      {
        status: "retrying",
        totalSize: 1_000_000,
        downloadedBytes: 100_000,
        speedBps: 4_000,
        connectionCount: 2,
        healthSummary: "taskDiagnostics.networkRetrying",
      },
      "2026-09-27T00:00:00.000Z",
    );

    expect(progress).toMatchObject({
      status: "retrying",
      downloadedBytes: 164_000,
      speedBps: 4_000,
      connectionCount: 2,
      completed: false,
    });
  });

  it("ignores inactive and unknown-size tasks", () => {
    const task = {
      status: "paused" as const,
      totalSize: 100,
      downloadedBytes: 100,
      speedBps: 0,
      connectionCount: 0,
      healthSummary: null,
    };

    expect(advanceBrowserMockProgress(task, "2026-09-27T00:00:00.000Z")).toBeNull();
    expect(advanceBrowserMockProgress({ ...task, status: "downloading", totalSize: 0 }, "now")).toBeNull();
  });
});
