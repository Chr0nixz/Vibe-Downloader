import type { Task as GeneratedTask, TaskFile as GeneratedTaskFile, TaskStatus } from "@/generated/bindings";

export type { TaskStatus };

/** App-facing task row; normalizes lossless IPC byte strings to UI numbers. */
export type TaskFile = Omit<GeneratedTaskFile, "totalSize" | "downloadedBytes"> & {
  totalSize: number;
  downloadedBytes: number;
};

export type Task = Omit<GeneratedTask, "totalSize" | "downloadedBytes" | "speedBps" | "files"> & {
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

export function normalizeTaskProtocol(protocol: unknown, url: unknown, fileName: unknown): string {
  if (typeof protocol === "string" && protocol.trim()) return protocol.trim().toLowerCase();

  const source = typeof url === "string" ? url.trim() : "";
  let scheme = "";
  let pathname = typeof fileName === "string" ? fileName : "";
  try {
    const parsed = new URL(source);
    scheme = parsed.protocol.replace(/:$/, "").toLowerCase();
    pathname ||= parsed.pathname;
  } catch {
    scheme = source.match(/^([a-z][a-z0-9+.-]*):/i)?.[1]?.toLowerCase() ?? "";
  }

  if (scheme === "magnet") return "bt";
  if (["ftp", "ftps", "sftp", "webdav", "webdavs"].includes(scheme)) return scheme;

  const extension = pathname.split(/[?#]/, 1)[0]?.split(".").pop()?.toLowerCase();
  if (extension === "torrent") return "bt";
  if (extension === "m3u" || extension === "m3u8") return "hls";
  if (extension === "mpd") return "dash";
  if (extension === "meta4" || extension === "metalink") return "metalink";
  if (scheme === "http" || scheme === "https") return "http";
  return "unknown";
}

export function normalizeTask(task: GeneratedTask | Task): Task {
  return {
    ...task,
    protocol: normalizeTaskProtocol(task.protocol, task.url, task.fileName),
    recoveryActions: task.recoveryActions ?? [],
    checksums: task.checksums ?? [],
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
