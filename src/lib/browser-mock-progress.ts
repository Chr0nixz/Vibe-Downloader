import type { Task } from "@/types/task";

type BrowserMockProgressTask = Pick<
  Task,
  "status" | "totalSize" | "downloadedBytes" | "speedBps" | "connectionCount" | "healthSummary"
>;

export type BrowserMockProgressResult = {
  status: Task["status"];
  downloadedBytes: number;
  speedBps: number;
  connectionCount: number;
  healthSummary: string | null;
  updatedAt: string;
  completed: boolean;
};

export function advanceBrowserMockProgress(
  task: BrowserMockProgressTask,
  updatedAt: string,
): BrowserMockProgressResult | null {
  if ((task.status !== "downloading" && task.status !== "retrying") || task.totalSize <= 0) return null;

  const step = Math.max(64_000, Math.floor(task.speedBps / 4));
  const downloadedBytes = Math.min(task.totalSize, Math.max(0, task.downloadedBytes) + step);
  const completed = downloadedBytes >= task.totalSize;

  return {
    status: completed ? "completed" : task.status,
    downloadedBytes,
    speedBps: completed ? 0 : task.speedBps,
    connectionCount: completed ? 0 : task.connectionCount,
    healthSummary: completed ? "taskDiagnostics.completed" : task.healthSummary,
    updatedAt,
    completed,
  };
}
