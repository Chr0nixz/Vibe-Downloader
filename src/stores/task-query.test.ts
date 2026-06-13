import { describe, expect, it } from "vitest";

import type { Task } from "@/types/task";

import {
  buildTaskCursorInput,
  failureKind,
  filterTasks,
  mergeTasksFromServer,
  taskFileType,
  type TaskQuerySnapshot,
} from "./task-query";

const baseQuery: TaskQuerySnapshot = {
  nav: "all",
  search: "",
  sortKey: "updated_at",
  sortDirection: "desc",
  filters: {
    fileType: "all",
    source: "all",
    failure: "all",
    resume: "all",
  },
  page: 0,
  pageSize: 50,
};

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
    status: "queued",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "manual",
    connectionCount: 0,
    speedBps: 0,
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
    files: [],
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

describe("task query helpers", () => {
  it("builds cursor input with the backend failureCategory field", () => {
    expect(
      buildTaskCursorInput({
        ...baseQuery,
        filters: { ...baseQuery.filters, failure: "http" },
      }),
    ).toMatchObject({
      failureCategory: "http",
      cursor: null,
      pageSize: 50,
    });
  });

  it("preserves live progress when refreshed task rows lag behind events", () => {
    const current = [
      makeTask({
        id: "task-1",
        status: "downloading",
        downloadedBytes: 72,
        speedBps: 2048,
        connectionCount: 4,
      }),
    ];
    const fresh = [
      makeTask({
        id: "task-1",
        status: "queued",
        downloadedBytes: 16,
        speedBps: 0,
        connectionCount: 0,
      }),
    ];

    expect(mergeTasksFromServer(current, fresh)[0]).toMatchObject({
      status: "downloading",
      downloadedBytes: 72,
      speedBps: 2048,
      connectionCount: 4,
    });
  });

  it("filters by nav, search, source, failure, and resume state", () => {
    const tasks = [
      makeTask({
        id: "task-http",
        fileName: "movie.mp4",
        sourceKey: "browser",
        status: "failed",
        failureCategory: "http",
        supportsResume: false,
        updatedAt: "2026-01-03T00:00:00.000Z",
      }),
      makeTask({
        id: "task-disk",
        fileName: "archive.zip",
        status: "failed",
        failureCategory: "disk_write",
        updatedAt: "2026-01-02T00:00:00.000Z",
      }),
      makeTask({
        id: "task-active",
        fileName: "movie-active.mp4",
        sourceKey: "browser",
        status: "downloading",
        updatedAt: "2026-01-04T00:00:00.000Z",
      }),
    ];

    expect(
      filterTasks(tasks, "failed", "movie", "updated_at", "desc", {
        fileType: "video",
        source: "browser",
        failure: "http",
        resume: "single_connection",
      }).map((task) => task.id),
    ).toEqual(["task-http"]);
  });

  it("classifies file and failure kinds from task metadata", () => {
    expect(taskFileType(makeTask({ fileName: "photo.webp" }))).toBe("image");
    expect(
      failureKind(
        makeTask({
          status: "failed",
          errorCode: "disk_write_failed",
        }),
      ),
    ).toBe("disk_write");
  });
});
