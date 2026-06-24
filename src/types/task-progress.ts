import type { TaskProgressPayload, TaskStatus } from "@/generated/bindings";

export type { TaskProgressPayload };

export function normalizeTaskProgressPayload(raw: unknown): TaskProgressPayload | null {
  if (!raw || typeof raw !== "object") return null;

  const record = raw as Record<string, unknown>;
  const taskId = record.taskId ?? record.task_id;
  if (typeof taskId !== "string" || taskId.length === 0) return null;

  const status = record.status;
  if (typeof status !== "string") return null;

  return {
    taskId,
    downloadedBytes: String(record.downloadedBytes ?? record.downloaded_bytes ?? 0),
    totalSize: String(record.totalSize ?? record.total_size ?? 0),
    speedBps: String(record.speedBps ?? record.speed_bps ?? 0),
    connectionCount: Number(record.connectionCount ?? record.connection_count ?? 0),
    status: status as TaskStatus,
  };
}
