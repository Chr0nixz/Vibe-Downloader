import { create } from "zustand";

import type { ListTasksCursorInput, ListTasksInput, TaskFilterOptions } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { parseByteCount } from "@/types/task";
import {
  normalizeTaskProgressPayload,
  type TaskProgressPayload,
} from "@/types/task-progress";

import {
  buildTaskCursorInput,
  buildTaskPageInput,
  failureKind,
  filterTasks,
  mergePagedTasks,
  mergeTasksFromServer,
  taskFileType,
} from "./task-query";
import {
  appendSpeedSample,
  pruneSpeedHistory,
  type SpeedHistoryByTaskId,
} from "./task-live-progress";

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

export interface TaskStats {
  all: number;
  active: number;
  queued: number;
  paused: number;
  completed: number;
  failed: number;
  totalSpeed: number;
  totalDownloaded: number;
  totalBytes: number;
  featuredTaskId: string | null;
}

export interface TaskFilters {
  fileType: FileTypeFilter;
  source: string;
  failure: string;
  resume: ResumeFilter;
}

interface TaskStore {
  tasks: Task[];
  taskIds: string[];
  taskById: Record<string, Task>;
  taskIndexById: Record<string, number>;
  taskStats: TaskStats;
  total: number;
  page: number;
  pageSize: number;
  nextCursor: string | null;
  hasMore: boolean;
  filterOptions: TaskFilterOptions;
  selectedId: string | null;
  selectedIds: string[];
  nav: NavFilter;
  search: string;
  sortKey: TaskSortKey;
  sortDirection: TaskSortDirection;
  filters: TaskFilters;
  detailOpen: boolean;
  expandedTaskIds: string[];
  completionFlashIds: string[];
  speedHistoryByTaskId: SpeedHistoryByTaskId;
  loading: boolean;
  error: string | null;
  setTasks: (tasks: Task[]) => void;
  setTaskPage: (tasks: Task[], total: number, page: number, pageSize: number, append?: boolean) => void;
  setTaskCursorPage: (
    tasks: Task[],
    totalEstimate: number,
    nextCursor: string | null,
    filterOptions: TaskFilterOptions,
    append?: boolean,
  ) => void;
  upsertTask: (task: Task) => void;
  patchTask: (payload: TaskProgressPayload | unknown) => void;
  patchTasksBatch: (payloads: Array<TaskProgressPayload | unknown>) => void;
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
  markCompletionFlash: (id: string) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

function indexTasks(tasks: Task[]): Record<string, number> {
  return Object.fromEntries(tasks.map((task, index) => [task.id, index]));
}

function mapTasksById(tasks: Task[]): Record<string, Task> {
  return Object.fromEntries(tasks.map((task) => [task.id, task]));
}

function calculateTaskStats(tasks: Task[]): TaskStats {
  const activeTasks: Task[] = [];
  let active = 0;
  let queued = 0;
  let paused = 0;
  let completed = 0;
  let failed = 0;
  let totalSpeed = 0;
  let totalDownloaded = 0;
  let totalBytes = 0;
  let fallbackTask: Task | null = null;

  for (const task of tasks) {
    if (task.status === "downloading" || task.status === "retrying") {
      active += 1;
      activeTasks.push(task);
      totalSpeed += task.speedBps;
      totalDownloaded += task.downloadedBytes;
      totalBytes += task.totalSize;
    }
    if (task.status === "queued") {
      queued += 1;
    }
    if (
      task.status === "paused" ||
      task.status === "queued" ||
      task.status === "waiting_network"
    ) {
      paused += 1;
    }
    if (task.status === "completed") {
      completed += 1;
    }
    if (task.status === "failed" || task.status === "needs_attention") {
      failed += 1;
    }
    if (
      !fallbackTask &&
      (
        task.status === "queued" ||
        task.status === "paused" ||
        task.status === "failed" ||
        task.status === "needs_attention" ||
        task.status === "waiting_network"
      )
    ) {
      fallbackTask = task;
    }
  }

  const featuredTask =
    activeTasks.reduce<Task | null>(
      (best, task) => (!best || task.speedBps > best.speedBps ? task : best),
      null,
    ) ?? fallbackTask;

  return {
    all: tasks.length,
    active,
    queued,
    paused,
    completed,
    failed,
    totalSpeed,
    totalDownloaded,
    totalBytes,
    featuredTaskId: featuredTask?.id ?? null,
  };
}

function taskCollections(tasks: Task[]) {
  return {
    tasks,
    taskIds: tasks.map((task) => task.id),
    taskById: mapTasksById(tasks),
    taskIndexById: indexTasks(tasks),
    taskStats: calculateTaskStats(tasks),
  };
}

function applyProgressToTask(task: Task, payload: TaskProgressPayload): Task {
  const downloadedBytes = parseByteCount(payload.downloadedBytes);
  const totalSize = parseByteCount(payload.totalSize);
  const speedBps = parseByteCount(payload.speedBps);

  return {
    ...task,
    downloadedBytes,
    totalSize,
    speedBps,
    connectionCount: payload.connectionCount,
    status: payload.status,
    files: (task.files ?? []).map((file) =>
      file.selected
        ? {
            ...file,
            downloadedBytes,
            totalSize,
            status: payload.status,
          }
        : file,
    ),
  };
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  taskIds: [],
  taskById: {},
  taskIndexById: {},
  taskStats: {
    all: 0,
    active: 0,
    queued: 0,
    paused: 0,
    completed: 0,
    failed: 0,
    totalSpeed: 0,
    totalDownloaded: 0,
    totalBytes: 0,
    featuredTaskId: null,
  },
  total: 0,
  page: 0,
  pageSize: 100,
  nextCursor: null,
  hasMore: false,
  filterOptions: {
    sources: [],
    failureCategories: [],
  },
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
  completionFlashIds: [],
  speedHistoryByTaskId: {},
  loading: true,
  error: null,
  setTasks: (tasks) =>
    set((state) => {
      const ids = new Set(tasks.map((task) => task.id));
      const speedHistoryByTaskId = pruneSpeedHistory(state.speedHistoryByTaskId, ids);
      return {
        ...taskCollections(tasks),
        total: tasks.length,
        page: 0,
        nextCursor: null,
        hasMore: false,
        selectedIds: state.selectedIds.filter((id) => ids.has(id)),
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  setTaskPage: (tasks, total, page, pageSize, append = false) =>
    set((state) => {
      const nextTasks = append ? mergePagedTasks(state.tasks, tasks) : tasks;
      const ids = new Set(nextTasks.map((task) => task.id));
      const speedHistoryByTaskId = pruneSpeedHistory(state.speedHistoryByTaskId, ids);
      return {
        ...taskCollections(nextTasks),
        total,
        page,
        pageSize,
        nextCursor: null,
        hasMore: nextTasks.length < total,
        selectedIds: state.selectedIds.filter((id) => ids.has(id)),
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  setTaskCursorPage: (tasks, totalEstimate, nextCursor, filterOptions, append = false) =>
    set((state) => {
      const nextTasks = append ? mergePagedTasks(state.tasks, tasks) : tasks;
      const ids = new Set(nextTasks.map((task) => task.id));
      const speedHistoryByTaskId = pruneSpeedHistory(state.speedHistoryByTaskId, ids);
      const knownTotal = Math.max(
        totalEstimate,
        nextTasks.length + (nextCursor ? 1 : 0),
      );
      return {
        ...taskCollections(nextTasks),
        total: knownTotal,
        page: append ? state.page + 1 : 0,
        nextCursor,
        hasMore: Boolean(nextCursor),
        filterOptions:
          append &&
          filterOptions.sources.length === 0 &&
          filterOptions.failureCategories.length === 0
            ? state.filterOptions
            : filterOptions,
        selectedIds: state.selectedIds.filter((id) => ids.has(id)),
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  upsertTask: (task) =>
    set((state) => {
      const index = state.taskIndexById[task.id] ?? -1;
      if (index < 0) {
        const tasks = [task, ...state.tasks];
        return {
          ...taskCollections(tasks),
          total: Math.max(state.total, tasks.length),
        };
      }
      const tasks = [...state.tasks];
      tasks[index] = {
        ...tasks[index],
        ...task,
        files: (task.files?.length ?? 0) > 0 ? task.files : tasks[index].files,
      };
      return {
        ...taskCollections(tasks),
      };
    }),
  patchTask: (raw) => get().patchTasksBatch([raw]),
  patchTasksBatch: (rawPayloads) => {
    const latestByTaskId = new Map<string, TaskProgressPayload>();
    for (const raw of rawPayloads) {
      const payload = normalizeTaskProgressPayload(raw);
      if (payload) latestByTaskId.set(payload.taskId, payload);
    }
    if (latestByTaskId.size === 0) return;

    const now = Date.now();
    set((state) => {
      let tasks = state.tasks;
      let taskById = state.taskById;
      let speedHistoryByTaskId = state.speedHistoryByTaskId;
      let changed = false;

      for (const payload of latestByTaskId.values()) {
        const index = state.taskIndexById[payload.taskId];
        if (index === undefined) continue;
        if (!changed) {
          tasks = [...state.tasks];
          taskById = { ...state.taskById };
          changed = true;
        }
        const nextTask = applyProgressToTask(tasks[index], payload);
        tasks[index] = nextTask;
        taskById[payload.taskId] = nextTask;
        speedHistoryByTaskId = appendSpeedSample(
          speedHistoryByTaskId,
          payload.taskId,
          { at: now, speedBps: parseByteCount(payload.speedBps) },
        );
      }

      if (!changed) return {};
      return {
        tasks,
        taskById,
        taskIds: state.taskIds,
        taskIndexById: state.taskIndexById,
        taskStats: calculateTaskStats(tasks),
        speedHistoryByTaskId,
      };
    });
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
  markCompletionFlash: (id) => {
    set((state) => ({
      completionFlashIds: state.completionFlashIds.includes(id)
        ? state.completionFlashIds
        : [...state.completionFlashIds, id],
    }));
    setTimeout(() => {
      set((state) => ({
        completionFlashIds: state.completionFlashIds.filter((taskId) => taskId !== id),
      }));
    }, 1800);
  },
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
}));

export function taskPageInput(page = useTaskStore.getState().page): ListTasksInput {
  return buildTaskPageInput(useTaskStore.getState(), page);
}

export function taskCursorInput(cursor: string | null = null): ListTasksCursorInput {
  return buildTaskCursorInput(useTaskStore.getState(), cursor);
}

export { failureKind, filterTasks, mergeTasksFromServer, taskFileType };
export type { SpeedSample } from "./task-live-progress";
