import type { Task as GeneratedTask, TaskStatus } from "@/generated/bindings";

export type { TaskStatus };

/** App-facing task row; normalizes lossless IPC byte strings to UI numbers. */
export type Task = Omit<
  GeneratedTask,
  "totalSize" | "downloadedBytes" | "speedBps"
> & {
  totalSize: number;
  downloadedBytes: number;
  speedBps: number;
};

function parseByteCount(value: string | number | null | undefined): number {
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  if (!value) return 0;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function normalizeTask(task: GeneratedTask): Task {
  return {
    ...task,
    totalSize: parseByteCount(task.totalSize),
    downloadedBytes: parseByteCount(task.downloadedBytes),
    speedBps: parseByteCount(task.speedBps),
  };
}

export { parseByteCount };
