import { describe, expect, it } from "vitest";

import type { Task } from "@/types/task";
import {
  hasInlineRecovery,
  inlineRecoveryActionsForTask,
  pauseWouldDiscardProgress,
  recoveryActionsForTask,
  rowShowsRetry,
  rowTransferMode,
} from "./row-recovery";

function task(overrides: Partial<Task> = {}): Task {
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
    downloadedBytes: 25,
    status: "downloading",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 4,
    speedBps: 1024,
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

describe("row recovery visibility", () => {
  it("marks an active non-resumable task with bytes on disk as destructive to pause", () => {
    expect(pauseWouldDiscardProgress(task({ supportsResume: false }))).toBe(true);
    expect(pauseWouldDiscardProgress(task({ supportsResume: true }))).toBe(false);
    expect(pauseWouldDiscardProgress(task({ supportsResume: false, downloadedBytes: 0 }))).toBe(false);
  });

  it("does not warn for paused or completed tasks even when resume is unavailable", () => {
    expect(pauseWouldDiscardProgress(task({ status: "paused", supportsResume: false }))).toBe(false);
    expect(pauseWouldDiscardProgress(task({ status: "completed", supportsResume: false }))).toBe(false);
  });

  it("hides resume and standalone retry when restart is the inline primary", () => {
    const failed = task({
      status: "failed",
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "open_folder"],
    });

    expect(hasInlineRecovery(failed)).toBe(true);
    expect(recoveryActionsForTask(failed)[0]).toBe("restart");
    expect(rowTransferMode(failed)).toBe("hidden");
    expect(rowShowsRetry(failed)).toBe(false);
  });

  it("hides resume when inline primary is retry so the solid button is the only retry", () => {
    const failed = task({
      status: "failed",
      errorMessage: "Network error",
      recoveryActions: ["retry"],
    });

    expect(rowTransferMode(failed)).toBe("hidden");
    expect(rowShowsRetry(failed)).toBe(false);
  });

  it("keeps a safe retry when a failed row has no recovery actions", () => {
    const failed = task({
      status: "failed",
      errorMessage: "Mystery failure",
      recoveryActions: [],
    });

    expect(hasInlineRecovery(failed)).toBe(false);
    expect(rowTransferMode(failed)).toBe("hidden");
    expect(rowShowsRetry(failed)).toBe(true);
  });

  it("keeps an open-folder utility action out of the inline recovery set", () => {
    const failed = task({
      status: "failed",
      errorMessage: "Network error",
      recoveryActions: ["open_folder"],
    });

    expect(inlineRecoveryActionsForTask(failed)).toEqual([]);
    expect(hasInlineRecovery(failed)).toBe(false);
    expect(rowShowsRetry(failed)).toBe(true);
  });

  it("still shows resume on paused tasks", () => {
    expect(rowTransferMode(task({ status: "paused" }))).toBe("resume");
    expect(rowShowsRetry(task({ status: "paused" }))).toBe(false);
  });
});
