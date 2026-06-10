import { create } from "zustand";

import type { ListTasksInput } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { parseByteCount } from "@/types/task";
import {
  normalizeTaskProgressPayload,
  type TaskProgressPayload,
} from "@/types/task-progress";

export interface SpeedSample {
  at: number;
  speedBps: number;
}

const SPEED_HISTORY_LIMIT = 60;

export type NavFilter =
  | "all"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "settings";

export type TaskSortKey =
  | "updated_at"
  | "created_at"
  | "file_size"
  | "progress"
  | "speed"
  | "status";

export type TaskSortDirection = "asc" | "desc";
export type FileTypeFilter = "all" | "archive" | "image" | "video" | "document" | "app" | "other";
export type ResumeFilter = "all" | "resumable" | "single_connection";

export interface TaskFilters {
  fileType: FileTypeFilter;
  source: string;
  failure: string;
  resume: ResumeFilter;
}

interface TaskStore {
  tasks: Task[];
  total: number;
  page: number;
  pageSize: number;
  hasMore: boolean;
  selectedId: string | null;
  selectedIds: string[];
  nav: NavFilter;
  search: string;
  sortKey: TaskSortKey;
  sortDirection: TaskSortDirection;
  filters: TaskFilters;
  detailOpen: boolean;
  expandedTaskIds: string[];
  speedHistoryByTaskId: Record<string, SpeedSample[]>;
  loading: boolean;
  error: string | null;
  setTasks: (tasks: Task[]) => void;
  setTaskPage: (tasks: Task[], total: number, page: number, pageSize: number, append?: boolean) => void;
  upsertTask: (task: Task) => void;
  patchTask: (payload: TaskProgressPayload | unknown) => void;
  selectTask: (id: string | null) => void;
  toggleTaskSelected: (id: string) => void;
  setTaskSelected: (id: string, selected: boolean) => void;
  setSelectedIds: (ids: string[]) => void;
  clearSelectedIds: () => void;
  setNav: (nav: NavFilter) => void;
  setSearch: (search: string) => void;
  setSort: (key: TaskSortKey, direction?: TaskSortDirection) => void;
  setFilters: (filters: Partial<TaskFilters>) => void;
  setDetailOpen: (open: boolean) => void;
  toggleTaskExpanded: (id: string) => void;
  collapseTask: (id: string) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],
  total: 0,
  page: 0,
  pageSize: 100,
  hasMore: false,
  selectedId: null,
  selectedIds: [],
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
  detailOpen: false,
  expandedTaskIds: [],
  speedHistoryByTaskId: {},
  loading: true,
  error: null,
  setTasks: (tasks) =>
    set((state) => {
      const ids = new Set(tasks.map((task) => task.id));
      const speedHistoryByTaskId = Object.fromEntries(
        Object.entries(state.speedHistoryByTaskId).filter(([id]) => ids.has(id)),
      );
      return {
        tasks,
        total: tasks.length,
        page: 0,
        hasMore: false,
        selectedIds: state.selectedIds.filter((id) => ids.has(id)),
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  setTaskPage: (tasks, total, page, pageSize, append = false) =>
    set((state) => {
      const nextTasks = append ? mergePagedTasks(state.tasks, tasks) : tasks;
      const ids = new Set(nextTasks.map((task) => task.id));
      const speedHistoryByTaskId = Object.fromEntries(
        Object.entries(state.speedHistoryByTaskId).filter(([id]) => ids.has(id)),
      );
      return {
        tasks: nextTasks,
        total,
        page,
        pageSize,
        hasMore: nextTasks.length < total,
        selectedIds: state.selectedIds.filter((id) => ids.has(id)),
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  upsertTask: (task) =>
    set((state) => {
      const index = state.tasks.findIndex((entry) => entry.id === task.id);
      if (index < 0) return { tasks: [task, ...state.tasks] };
      const tasks = [...state.tasks];
      tasks[index] = {
        ...tasks[index],
        ...task,
        files: task.files.length > 0 ? task.files : tasks[index].files,
      };
      return { tasks };
    }),
  patchTask: (raw) => {
    const payload = normalizeTaskProgressPayload(raw);
    if (!payload) return;
    const speedBps = parseByteCount(payload.speedBps);
    const sample: SpeedSample = { at: Date.now(), speedBps };

    set((state) => ({
      tasks: state.tasks.map((task) =>
        task.id === payload.taskId
          ? {
              ...task,
              downloadedBytes: parseByteCount(payload.downloadedBytes),
              totalSize: parseByteCount(payload.totalSize),
              speedBps,
              connectionCount: payload.connectionCount,
              status: payload.status,
              files: task.files.map((file) =>
                file.selected
                  ? {
                      ...file,
                      downloadedBytes: parseByteCount(payload.downloadedBytes),
                      totalSize: parseByteCount(payload.totalSize),
                      status: payload.status,
                    }
                  : file,
              ),
            }
          : task,
      ),
      speedHistoryByTaskId: {
        ...state.speedHistoryByTaskId,
        [payload.taskId]: [
          ...(state.speedHistoryByTaskId[payload.taskId] ?? []),
          sample,
        ].slice(-SPEED_HISTORY_LIMIT),
      },
    }));
  },
  selectTask: (id) => set({ selectedId: id }),
  toggleTaskSelected: (id) =>
    set((state) => ({
      selectedIds: state.selectedIds.includes(id)
        ? state.selectedIds.filter((taskId) => taskId !== id)
        : [...state.selectedIds, id],
    })),
  setTaskSelected: (id, selected) =>
    set((state) => ({
      selectedIds: selected
        ? Array.from(new Set([...state.selectedIds, id]))
        : state.selectedIds.filter((taskId) => taskId !== id),
    })),
  setSelectedIds: (ids) => set({ selectedIds: Array.from(new Set(ids)) }),
  clearSelectedIds: () => set({ selectedIds: [] }),
  setNav: (nav) => set({ nav }),
  setSearch: (search) => set({ search }),
  setSort: (key, direction) =>
    set((state) => ({
      sortKey: key,
      sortDirection:
        direction ??
        (state.sortKey === key && state.sortDirection === "desc" ? "asc" : "desc"),
    })),
  setFilters: (filters) =>
    set((state) => ({ filters: { ...state.filters, ...filters } })),
  setDetailOpen: (open) => set({ detailOpen: open }),
  toggleTaskExpanded: (id) =>
    set((state) => ({
      expandedTaskIds: state.expandedTaskIds.includes(id)
        ? state.expandedTaskIds.filter((taskId) => taskId !== id)
        : [...state.expandedTaskIds, id],
    })),
  collapseTask: (id) =>
    set((state) => ({
      expandedTaskIds: state.expandedTaskIds.filter((taskId) => taskId !== id),
    })),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
}));

function mergePagedTasks(current: Task[], incoming: Task[]): Task[] {
  const byId = new Map(current.map((task) => [task.id, task] as const));
  const order = current.map((task) => task.id);
  for (const task of incoming) {
    if (!byId.has(task.id)) order.push(task.id);
    byId.set(task.id, task);
  }
  return order.map((id) => byId.get(id)).filter((task): task is Task => Boolean(task));
}

export function taskPageInput(page = useTaskStore.getState().page): ListTasksInput {
  const state = useTaskStore.getState();
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

/** Keep in-flight progress when a DB refresh lags behind live events. */
export function mergeTasksFromServer(current: Task[], fresh: Task[]): Task[] {
  const liveById = new Map(
    current
      .filter(
        (task) => task.status === "downloading" || task.status === "retrying",
      )
      .map((task) => [task.id, task] as const),
  );

  return fresh.map((task) => {
    const live = liveById.get(task.id);
    if (!live) return task;
    return {
      ...task,
      downloadedBytes: Math.max(task.downloadedBytes, live.downloadedBytes),
      speedBps: live.speedBps > 0 ? live.speedBps : task.speedBps,
      connectionCount:
        live.connectionCount > 0 ? live.connectionCount : task.connectionCount,
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
      task.url.toLowerCase().includes(query)
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

function compareTasks(
  a: Task,
  b: Task,
  sortKey: TaskSortKey,
  direction: TaskSortDirection,
): number {
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
    case "updated_at":
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

export function taskFileType(task: Task): FileTypeFilter {
  const name = task.fileName.toLowerCase();
  const contentType = task.contentType?.toLowerCase() ?? "";
  if (
    contentType.includes("zip") ||
    /\.(zip|rar|7z|tar|gz|bz2|xz)$/i.test(name)
  ) {
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
