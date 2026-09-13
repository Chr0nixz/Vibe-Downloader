import { beforeEach, describe, expect, it } from "vitest";

import type { Task } from "@/types/task";

import { useTaskDataStore } from "./task-data-store";

function makeTask(id: string): Task {
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
    status: "queued",
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

function seedTasks(ids: string[]) {
  const tasks = ids.map(makeTask);
  const taskById: Record<string, Task> = {};
  const taskIndexById: Record<string, number> = {};
  tasks.forEach((task, index) => {
    taskById[task.id] = task;
    taskIndexById[task.id] = index;
  });
  useTaskDataStore.setState({ tasks, taskById, taskIds: ids, taskIndexById });
  return tasks;
}

describe("task-data-store reorder rollback (UX-20)", () => {
  beforeEach(() => {
    useTaskDataStore.setState({
      tasks: [],
      taskIds: [],
      taskById: {},
      taskIndexById: {},
      total: 0,
      nextCursor: null,
      hasMore: false,
      loading: false,
      error: null,
      filterOptions: { sources: [], failureCategories: [] },
    });
  });

  it("reorders affected tasks in place and returns a rollback handle", () => {
    seedTasks(["a", "b", "c"]);

    // Production callers pass the full same-priority subset in the new order
    // (handleReorder), so every affected slot is remapped.
    const rollback = useTaskDataStore.getState().reorderTasksLocally(["c", "a", "b"]);

    expect(rollback).toBeTypeOf("function");
    expect(useTaskDataStore.getState().taskIds).toEqual(["c", "a", "b"]);
    expect(useTaskDataStore.getState().taskIndexById).toEqual({ a: 1, b: 2, c: 0 });
    // The task objects themselves are untouched, only their positions moved.
    expect(useTaskDataStore.getState().tasks[0]).toEqual(makeTask("c"));
    expect(useTaskDataStore.getState().tasks[1]).toEqual(makeTask("a"));

    rollback?.();
    expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b", "c"]);
    expect(useTaskDataStore.getState().taskIndexById).toEqual({ a: 0, b: 1, c: 2 });
  });

  it("rollback restores the exact pre-reorder snapshot even if the list changed since", () => {
    seedTasks(["a", "b", "c"]);
    const snapshot = useTaskDataStore.getState().tasks;

    const rollback = useTaskDataStore.getState().reorderTasksLocally(["c", "b"]);
    expect(useTaskDataStore.getState().taskIds).toEqual(["a", "c", "b"]);
    // A third task arrives between the optimistic move and the rollback.
    const extra = makeTask("d");
    useTaskDataStore.setState((state) => ({
      tasks: [...state.tasks, extra],
      taskIds: [...state.taskIds, "d"],
    }));

    rollback?.();

    // The handle restores the pre-reorder snapshot, not "undo one step": the
    // caller re-fetches the authoritative server order right after (UX-20).
    expect(useTaskDataStore.getState().tasks).toBe(snapshot);
    expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b", "c"]);
  });

  it("returns null and keeps state untouched for an empty id list", () => {
    seedTasks(["a", "b"]);
    const before = useTaskDataStore.getState().tasks;

    const rollback = useTaskDataStore.getState().reorderTasksLocally([]);

    expect(rollback).toBeNull();
    expect(useTaskDataStore.getState().tasks).toBe(before);
    expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b"]);
  });
});
