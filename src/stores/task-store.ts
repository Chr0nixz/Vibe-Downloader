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
  type SpeedSample,
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

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],
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
        tasks,
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
        tasks: nextTasks,
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
        tasks: nextTasks,
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
      const index = state.tasks.findIndex((entry) => entry.id === task.id);
      if (index < 0) return { tasks: [task, ...state.tasks] };
      const tasks = [...state.tasks];
      tasks[index] = {
        ...tasks[index],
        ...task,
        files: (task.files?.length ?? 0) > 0 ? task.files : tasks[index].files,
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
              files: (task.files ?? []).map((file) =>
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
      speedHistoryByTaskId: appendSpeedSample(
        state.speedHistoryByTaskId,
        payload.taskId,
        sample,
      ),
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
