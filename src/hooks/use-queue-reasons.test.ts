import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { useQueueReasons } from "./use-queue-reasons";

const getSchedulerSnapshot = vi.fn();

vi.mock("@/lib/tauri", () => ({
  getSchedulerSnapshot: (...args: unknown[]) => getSchedulerSnapshot(...args),
}));

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "hidden", {
    configurable: true,
    get: () => hidden,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

function makeTask(id: string, status: Task["status"]): Task {
  const now = "2026-01-01T00:00:00.000Z";
  return {
    id,
    url: `https://example.com/${id}`,
    finalUrl: null,
    protocol: "http",
    taskKind: "single_file",
    fileName: `${id}.bin`,
    saveDir: "D:\\Downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 100,
    downloadedBytes: 0,
    status,
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 0,
    speedBps: 0,
    taskSpeedLimitBps: null,
    priority: "normal",
    queuePosition: "0",
    categoryKey: null,
    obeySchedule: true,
    healthSummary: null,
    errorMessage: null,
    errorCode: null,
    recoveryActions: [],
    retryAfterAt: null,
    failureCategory: null,
    expectedHashSha256: null,
    actualHashSha256: null,
    hashStatus: "not_requested",
    hashError: null,
    hashVerifiedAt: null,
    checksums: [],
    files: [],
    createdAt: now,
    updatedAt: now,
  };
}

function seedTasks(tasks: Task[]) {
  useTaskDataStore.setState({
    tasks,
    taskIds: tasks.map((task) => task.id),
    taskById: Object.fromEntries(tasks.map((task) => [task.id, task])),
  });
}

describe("useQueueReasons (PERF-19 / R26-P04)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setHidden(false);
    vi.clearAllMocks();
    getSchedulerSnapshot.mockResolvedValue({ decisions: [] });
    useTaskDataStore.setState({
      tasks: [],
      taskIds: [],
      taskById: {},
      taskIndexById: {},
      total: 0,
      viewReloadToken: 0,
      nextCursor: null,
      hasMore: false,
      loading: false,
      error: null,
      filterOptions: { sources: [], failureCategories: [] },
    });
    useTaskUIStore.setState({
      nav: "all",
      search: "",
      selectedId: null,
      selectedIds: [],
      pendingDeleteIds: [],
    });
  });

  afterEach(() => {
    setHidden(false);
    vi.useRealTimers();
  });

  it("does not poll at all when no task is queued", async () => {
    seedTasks([makeTask("a", "downloading"), makeTask("b", "completed")]);
    renderHook(() => useQueueReasons(["a", "b"]));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(getSchedulerSnapshot).not.toHaveBeenCalled();
  });

  it("polls while visible and stops while the document is hidden", async () => {
    seedTasks([makeTask("q1", "queued"), makeTask("a", "completed")]);
    renderHook(() => useQueueReasons(["q1", "a"]));

    // Immediate load on mount.
    await act(async () => {
      await Promise.resolve();
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(1);
    expect(getSchedulerSnapshot).toHaveBeenCalledWith(["q1"]);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(3);

    act(() => setHidden(true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(3);

    await act(async () => {
      setHidden(false);
    });
    // Re-show refreshes immediately instead of waiting a full interval.
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(4);
  });

  it("does not overlap a snapshot request that outlasts the interval", async () => {
    let release: (() => void) | undefined;
    getSchedulerSnapshot.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => resolve({ decisions: [] });
        }),
    );
    seedTasks([makeTask("q1", "queued")]);
    renderHook(() => useQueueReasons(["q1"]));

    await act(async () => {
      await Promise.resolve();
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(1);

    // Interval ticks while the first request is still in flight must not stack.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(40_000);
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(1);

    await act(async () => {
      release?.();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(getSchedulerSnapshot).toHaveBeenCalledTimes(2);
  });
});
