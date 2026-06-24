import type { ListTasksCursorInput, ListTasksInput } from "@/generated/bindings";
import { sanitizeUrlForDisplay } from "@/lib/utils";
import type { Task } from "@/types/task";

import type { FileTypeFilter, NavFilter, TaskFilters, TaskSortDirection, TaskSortKey } from "./task-data-store";

export interface TaskQuerySnapshot {
  nav: NavFilter;
  search: string;
  sortKey: TaskSortKey;
  sortDirection: TaskSortDirection;
  filters: TaskFilters;
  page: number;
  pageSize: number;
}

export function buildTaskPageInput(state: TaskQuerySnapshot, page = state.page): ListTasksInput {
  return {
    nav: state.nav,
    search: state.search,
    sortKey: state.sortKey,
    sortDirection: state.sortDirection,
    fileType: state.filters.fileType,
    source: state.filters.source,
    failure: state.filters.failure,
    resume: state.filters.resume,
    page,
    pageSize: state.pageSize,
  };
}

export function buildTaskCursorInput(state: TaskQuerySnapshot, cursor: string | null = null): ListTasksCursorInput {
  return {
    nav: state.nav,
    search: state.search,
    sortKey: state.sortKey,
    sortDirection: state.sortDirection,
    fileType: state.filters.fileType,
    source: state.filters.source,
    failureCategory: state.filters.failure,
    resume: state.filters.resume,
    cursor,
    pageSize: state.pageSize,
  };
}

export function mergePagedTasks(current: Task[], incoming: Task[]): Task[] {
  const byId = new Map(current.map((task) => [task.id, task] as const));
  const order = current.map((task) => task.id);
  for (const task of incoming) {
    if (!byId.has(task.id)) order.push(task.id);
    byId.set(task.id, task);
  }
  return order.map((id) => byId.get(id)).filter((task): task is Task => Boolean(task));
}

/** Keep in-flight progress when a DB refresh lags behind live events. */
export function mergeTasksFromServer(current: Task[], fresh: Task[]): Task[] {
  const liveById = new Map(
    current
      .filter((task) => task.status === "downloading" || task.status === "retrying")
      .map((task) => [task.id, task] as const),
  );

  return fresh.map((task) => {
    const live = liveById.get(task.id);
    if (!live) return task;
    return {
      ...task,
      downloadedBytes: Math.max(task.downloadedBytes, live.downloadedBytes),
      speedBps: live.speedBps > 0 ? live.speedBps : task.speedBps,
      connectionCount: live.connectionCount > 0 ? live.connectionCount : task.connectionCount,
      status: live.status,
    };
  });
}

export function filterTasks(
  tasks: Task[],
  nav: NavFilter,
  search: string,
  sortKey: TaskSortKey = "updated_at",
  sortDirection: TaskSortDirection = "desc",
  filters: TaskFilters = {
    fileType: "all",
    source: "all",
    failure: "all",
    resume: "all",
  },
): Task[] {
  const query = search.trim().toLowerCase();

  const filtered = tasks.filter((task) => {
    if (nav === "downloading" && task.status !== "downloading" && task.status !== "retrying") {
      return false;
    }
    if (nav === "paused" && task.status !== "paused") return false;
    if (nav === "completed" && task.status !== "completed") return false;
    if (nav === "failed" && task.status !== "failed" && task.status !== "needs_attention") {
      return false;
    }
    if (nav === "settings") return false;

    if (!query) return true;
    return (
      task.fileName.toLowerCase().includes(query) ||
      task.sourceKey.toLowerCase().includes(query) ||
      sanitizeUrlForDisplay(task.url).toLowerCase().includes(query)
    );
  });

  return filtered
    .filter((task) => {
      if (filters.fileType !== "all" && taskFileType(task) !== filters.fileType) {
        return false;
      }
      if (filters.source !== "all" && task.sourceKey !== filters.source) {
        return false;
      }
      if (filters.failure !== "all" && failureKind(task) !== filters.failure) {
        return false;
      }
      if (filters.resume === "resumable" && !task.supportsResume) return false;
      if (filters.resume === "single_connection" && task.supportsResume) return false;
      return true;
    })
    .sort((a, b) => compareTasks(a, b, sortKey, sortDirection));
}

export function taskFileType(task: Task): FileTypeFilter {
  const name = task.fileName.toLowerCase();
  const contentType = task.contentType?.toLowerCase() ?? "";
  if (contentType.includes("zip") || /\.(zip|rar|7z|tar|gz|bz2|xz)$/i.test(name)) {
    return "archive";
  }
  if (contentType.startsWith("image/") || /\.(png|jpg|jpeg|gif|webp|avif|svg)$/i.test(name)) {
    return "image";
  }
  if (contentType.startsWith("video/") || /\.(mp4|mkv|mov|webm|avi)$/i.test(name)) {
    return "video";
  }
  if (/\.(pdf|doc|docx|xls|xlsx|ppt|pptx|txt|md)$/i.test(name)) {
    return "document";
  }
  if (/\.(exe|msi|dmg|pkg|deb|rpm|appimage)$/i.test(name)) {
    return "app";
  }
  return "other";
}

export function failureKind(task: Task): string {
  if (task.status !== "failed" && task.status !== "needs_attention") return "none";
  if (task.failureCategory) return task.failureCategory;
  if (task.errorCode) {
    if (task.errorCode === "remote_changed") return "remote_changed";
    if (task.errorCode === "resume_unavailable") return "resume_unavailable";
    if (task.errorCode.startsWith("temp_file")) return "temp_file";
    if (task.errorCode === "disk_write_failed") return "disk_write";
    if (task.errorCode.startsWith("http_") || task.errorCode === "server_rate_limited") {
      return "http";
    }
    return task.errorCode;
  }
  const message = (task.errorMessage ?? task.healthSummary ?? "").toLowerCase();
  if (message.includes("remote file changed")) return "remote_changed";
  if (message.includes("resume")) return "resume_unavailable";
  if (message.includes("temporary file")) return "temp_file";
  if (message.includes("disk") || message.includes("write")) return "disk_write";
  if (message.includes("http") || /\b(403|404|429|500|502|503)\b/.test(message)) {
    return "http";
  }
  return "other";
}

function compareTasks(a: Task, b: Task, sortKey: TaskSortKey, direction: TaskSortDirection): number {
  const multiplier = direction === "asc" ? 1 : -1;
  let result = 0;
  switch (sortKey) {
    case "created_at":
      result = Date.parse(a.createdAt) - Date.parse(b.createdAt);
      break;
    case "file_size":
      result = a.totalSize - b.totalSize;
      break;
    case "progress":
      result = progressValue(a) - progressValue(b);
      break;
    case "speed":
      result = a.speedBps - b.speedBps;
      break;
    case "status":
      result = statusRank(a.status) - statusRank(b.status);
      break;
    default:
      result = Date.parse(a.updatedAt) - Date.parse(b.updatedAt);
      break;
  }
  return result === 0 ? a.fileName.localeCompare(b.fileName) : result * multiplier;
}

function progressValue(task: Task): number {
  return task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
}

function statusRank(status: Task["status"]): number {
  switch (status) {
    case "downloading":
      return 0;
    case "retrying":
      return 1;
    case "queued":
      return 2;
    case "paused":
      return 3;
    case "waiting_network":
      return 4;
    case "needs_attention":
      return 5;
    case "failed":
      return 6;
    case "completed":
      return 7;
    default:
      return 8;
  }
}
