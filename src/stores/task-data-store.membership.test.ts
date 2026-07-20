import { beforeEach, describe, expect, it } from "vitest";

import type { Task } from "@/types/task";

import { useTaskDataStore } from "./task-data-store";
import { useTaskUIStore } from "./task-ui-store";

function makeTask(overrides: Partial<Task> = {}): Task {
  const now = "2026-01-01T00:00:00.000Z";
  return {
    id: "task-1",
    url: "https://example.com/file.bin",
    finalUrl: null,
    protocol: "http",
    taskKind: "single_file",
    fileName: "file.bin",
    saveDir: "D:\\Downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 100,
    downloadedBytes: 0,
    status: "completed",
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
    ...overrides,
  };
}

describe("task-data-store membership (ARC-08)", () => {
  beforeEach(() => {
    useTaskUIStore.setState({
      nav: "completed",
      search: "",
      filters: { fileType: "all", source: "all", failure: "all", resume: "all" },
      sortKey: "updated_at",
      sortDirection: "desc",
    });
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
  });

  it("removes a task from the completed view when status becomes queued", () => {
    const completed = makeTask({ id: "c1", status: "completed" });
    useTaskDataStore.getState().setTaskCursorPage([completed], 1, null, {
      sources: [],
      failureCategories: [],
    });
    expect(useTaskDataStore.getState().taskIds).toEqual(["c1"]);

    useTaskDataStore.getState().upsertTask({ ...completed, status: "queued" });
    expect(useTaskDataStore.getState().taskIds).toEqual([]);
    expect(useTaskDataStore.getState().taskById.c1?.status).toBe("queued");
  });

  it("does not prepend a task that fails the current search filter", () => {
    useTaskUIStore.setState({ nav: "all", search: "report" });
    const tokenBefore = useTaskDataStore.getState().viewReloadToken;
    useTaskDataStore.getState().upsertTask(makeTask({ id: "other", fileName: "other.bin", status: "queued" }));
    expect(useTaskDataStore.getState().taskIds).toEqual([]);
    expect(useTaskDataStore.getState().viewReloadToken).toBe(tokenBefore);
    expect(useTaskDataStore.getState().taskById.other).toBeDefined();
  });

  it("requests a view reload when a matching task is not already in the page", () => {
    useTaskUIStore.setState({ nav: "all", search: "" });
    const tokenBefore = useTaskDataStore.getState().viewReloadToken;
    useTaskDataStore.getState().upsertTask(makeTask({ id: "new", status: "queued" }));
    expect(useTaskDataStore.getState().taskIds).toEqual([]);
    expect(useTaskDataStore.getState().viewReloadToken).toBe(tokenBefore + 1);
  });

  it("evicts from the queue view after a progress patch changes status", () => {
    useTaskUIStore.setState({ nav: "queue", search: "" });
    const queued = makeTask({ id: "q1", status: "queued" });
    useTaskDataStore.getState().setTaskCursorPage([queued], 1, null, {
      sources: [],
      failureCategories: [],
    });

    const result = useTaskDataStore.getState().patchTasksBatch([
      {
        taskId: "q1",
        downloadedBytes: "10",
        totalSize: "100",
        speedBps: "0",
        connectionCount: 0,
        status: "completed",
      },
    ]);

    expect(result.statusTransitions).toEqual([
      expect.objectContaining({
        taskId: "q1",
        previousStatus: "queued",
        task: expect.objectContaining({ id: "q1", status: "completed" }),
      }),
    ]);
    expect(useTaskDataStore.getState().taskIds).toEqual([]);
    expect(useTaskDataStore.getState().taskById.q1?.status).toBe("completed");
  });

  it("does not rewrite per-file downloadedBytes on progress ticks", () => {
    const files = [
      {
        id: "f1",
        taskId: "multi",
        relativePath: "a.bin",
        fileName: "a.bin",
        saveDir: "D:\\Downloads",
        tempPath: null,
        finalPath: null,
        totalSize: 50,
        downloadedBytes: 10,
        selected: true,
        status: "downloading" as const,
        contentType: null,
      },
      {
        id: "f2",
        taskId: "multi",
        relativePath: "b.bin",
        fileName: "b.bin",
        saveDir: "D:\\Downloads",
        tempPath: null,
        finalPath: null,
        totalSize: 50,
        downloadedBytes: 20,
        selected: true,
        status: "downloading" as const,
        contentType: null,
      },
    ];
    const task = makeTask({
      id: "multi",
      status: "downloading",
      downloadedBytes: 30,
      totalSize: 100,
      files,
    });
    useTaskUIStore.setState({ nav: "all", search: "" });
    useTaskDataStore.getState().setTaskCursorPage([task], 1, null, {
      sources: [],
      failureCategories: [],
    });

    const result = useTaskDataStore.getState().patchTasksBatch([
      {
        taskId: "multi",
        downloadedBytes: "60",
        totalSize: "100",
        speedBps: "1024",
        connectionCount: 2,
        status: "downloading",
      },
    ]);

    expect(result.statusTransitions).toEqual([]);
    const next = useTaskDataStore.getState().taskById.multi;
    expect(next?.downloadedBytes).toBe(60);
    expect(next?.files?.[0]?.downloadedBytes).toBe(10);
    expect(next?.files?.[1]?.downloadedBytes).toBe(20);
  });
});
