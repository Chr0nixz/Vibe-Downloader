import type {
  Task as GeneratedTask,
  TaskFile as GeneratedTaskFile,
  TaskStatus,
} from "@/generated/bindings";

export type { TaskStatus };

/** App-facing task row; normalizes lossless IPC byte strings to UI numbers. */
export type TaskFile = Omit<
  GeneratedTaskFile,
  "totalSize" | "downloadedBytes"
> & {
  totalSize: number;
  downloadedBytes: number;
};

export type Task = Omit<
  GeneratedTask,
  "totalSize" | "downloadedBytes" | "speedBps" | "files"
> & {
  totalSize: number;
  downloadedBytes: number;
  speedBps: number;
  files: TaskFile[];
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
    recoveryActions: task.recoveryActions ?? [],
    totalSize: parseByteCount(task.totalSize),
    downloadedBytes: parseByteCount(task.downloadedBytes),
    speedBps: parseByteCount(task.speedBps),
    files: (task.files ?? []).map((file) => ({
      ...file,
      totalSize: parseByteCount(file.totalSize),
      downloadedBytes: parseByteCount(file.downloadedBytes),
    })),
  };
}

export { parseByteCount };
