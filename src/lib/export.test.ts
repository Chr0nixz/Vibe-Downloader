import { describe, expect, it } from "vitest";
import type { Task } from "@/types/task";
import { serializeTasks } from "./export";

function makeTask(overrides: Partial<Task> = {}): Task {
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
    downloadedBytes: 40,
    status: "queued",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "manual",
    connectionCount: 1,
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
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

describe("serializeTasks", () => {
  it("writes JSON rows for the selected tasks", () => {
    const json = serializeTasks([makeTask()], "json");
    const rows = JSON.parse(json) as Array<{ id: string; fileName: string }>;
    expect(rows).toHaveLength(1);
    expect(rows[0]?.id).toBe("task-1");
    expect(rows[0]?.fileName).toBe("file.bin");
  });

  it("escapes CSV fields that contain commas and quotes", () => {
    const csv = serializeTasks([makeTask({ fileName: 'report, "final".bin', errorMessage: "line1\nline2" })], "csv");
    expect(csv).toContain('"report, ""final"".bin"');
    expect(csv).toContain('"line1\nline2"');
    expect(csv.startsWith("id,url,fileName,")).toBe(true);
  });
});
