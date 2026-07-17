import { create } from "zustand";

import type { ListTasksCursorInput, ListTasksInput, TaskFilterOptions, TaskStatsSnapshot } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { parseByteCount } from "@/types/task";
import { normalizeTaskProgressPayload, type TaskProgressPayload } from "@/types/task-progress";
import { useSpeedHistoryStore } from "./speed-history-store";
import { buildTaskCursorInput, buildTaskPageInput, failureKind, mergePagedTasks } from "./task-query";
import { useTaskUIStore } from "./task-ui-store";

/* ── Types ── */

export type NavFilter =
  | "all"
  | "downloading"
  | "queue"
  | "attention"
  | "paused"
  | "completed"
  | "failed"
  | "settings"
  | "about";

export type TaskSortKey = "updated_at" | "created_at" | "file_size" | "progress" | "speed" | "status" | "queue_order";

export type TaskSortDirection = "asc" | "desc";
export type FileTypeFilter = "all" | "archive" | "image" | "video" | "document" | "app" | "other";
export type ResumeFilter = "all" | "resumable" | "single_connection";

export interface TaskStats {
  all: number;
  active: number;
  queued: number;
  attention: number;
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

/* ── Helpers ── */

export const EMPTY_TASK_STATS: TaskStats = {
  all: 0,
  active: 0,
  queued: 0,
  attention: 0,
  paused: 0,
  completed: 0,
  failed: 0,
  totalSpeed: 0,
  totalDownloaded: 0,
  totalBytes: 0,
  featuredTaskId: null,
};

export function indexTasks(tasks: Task[]): Record<string, number> {
  return Object.fromEntries(tasks.map((task, index) => [task.id, index]));
}

export function mapTasksById(tasks: Task[]): Record<string, Task> {
  return Object.fromEntries(tasks.map((task) => [task.id, task]));
}

export function calculateTaskStats(tasks: Task[]): TaskStats {
  const activeTasks: Task[] = [];
  let active = 0;
  let queued = 0;
  let attention = 0;
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
    if (task.status === "needs_attention") {
      attention += 1;
    }
    if (task.status === "paused") {
      paused += 1;
    }
    if (task.status === "completed") {
      completed += 1;
    }
    if (task.status === "failed") {
      failed += 1;
    }
    if (
      !fallbackTask &&
      (task.status === "queued" ||
        task.status === "paused" ||
        task.status === "failed" ||
        task.status === "needs_attention" ||
        task.status === "waiting_network")
    ) {
      fallbackTask = task;
    }
  }

  const featuredTask =
    activeTasks.reduce<Task | null>((best, task) => (!best || task.speedBps > best.speedBps ? task : best), null) ??
    fallbackTask;

  return {
    all: tasks.length,
    active,
    queued,
    attention,
    paused,
    completed,
    failed,
    totalSpeed,
    totalDownloaded,
    totalBytes,
    featuredTaskId: featuredTask?.id ?? null,
  };
}

export function normalizeTaskStatsSnapshot(stats: TaskStats | TaskStatsSnapshot): TaskStats {
  return {
    all: Number(stats.all) || 0,
    active: Number(stats.active) || 0,
    queued: Number(stats.queued) || 0,
    attention: Number(stats.attention) || 0,
    paused: Number(stats.paused) || 0,
    completed: Number(stats.completed) || 0,
    failed: Number(stats.failed) || 0,
    totalSpeed: parseByteCount(stats.totalSpeed),
    totalDownloaded: parseByteCount(stats.totalDownloaded),
    totalBytes: parseByteCount(stats.totalBytes),
    featuredTaskId: stats.featuredTaskId ?? null,
  };
}

function taskCollections(tasks: Task[]) {
  return {
    tasks,
    taskIds: tasks.map((task) => task.id),
    taskById: mapTasksById(tasks),
    taskIndexById: indexTasks(tasks),
    taskStats: calculateTaskStats(tasks),
    failureOptions: computeFailureOptions(tasks),
  };
}

/** E-3: Compute failure category options from the task list (called only on task load/status change, not on progress ticks) */
function computeFailureOptions(tasks: Task[]): string[] {
  return Array.from(new Set(tasks.map(failureKind).filter((kind) => kind !== "none"))).sort();
}

function applyProgressToTask(task: Task, payload: TaskProgressPayload): Task {
  const downloadedBytes = parseByteCount(payload.downloadedBytes);
  const totalSize = parseByteCount(payload.totalSize);
  const speedBps = parseByteCount(payload.speedBps);

  // Zero-delta fast path: if no field actually changed, return the same
  // reference so callers can skip the patch entirely. This prevents
  // patchTasksBatch from allocating new tasks/taskById/taskStats objects
  // when the backend re-sends unchanged progress values.
  if (
    task.downloadedBytes === downloadedBytes &&
    task.totalSize === totalSize &&
    task.speedBps === speedBps &&
    task.connectionCount === payload.connectionCount &&
    task.status === payload.status
  ) {
    return task;
  }

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

/** Map a task status to the stat counter fields it contributes to. */
function statusCounterKeys(
  status: Task["status"],
): Array<"active" | "queued" | "attention" | "paused" | "completed" | "failed"> {
  const keys: Array<"active" | "queued" | "attention" | "paused" | "completed" | "failed"> = [];
  if (status === "downloading" || status === "retrying") keys.push("active");
  if (status === "queued") keys.push("queued");
  if (status === "needs_attention") keys.push("attention");
  if (status === "paused") keys.push("paused");
  if (status === "completed") keys.push("completed");
  if (status === "failed") keys.push("failed");
  return keys;
}

/* ── Store interface ── */

interface TaskDataStore {
  tasks: Task[];
  taskIds: string[];
  taskById: Record<string, Task>;
  taskIndexById: Record<string, number>;
  taskStats: TaskStats;
  failureOptions: string[];
  globalTaskStats: TaskStats | null;
  total: number;
  page: number;
  pageSize: number;
  nextCursor: string | null;
  hasMore: boolean;
  filterOptions: TaskFilterOptions;
  expandedTaskIds: string[];
  completionFlashIds: string[];
  loading: boolean;
  error: string | null;
  setTasks: (tasks: Task[]) => void;
  setTaskPage: (tasks: Task[], total: number, page: number, pageSize: number, append?: boolean) => void;
  setTaskCursorPage: (
    tasks: Task[],
    minimumTotal: number,
    nextCursor: string | null,
    filterOptions: TaskFilterOptions,
    append?: boolean,
  ) => void;
  upsertTask: (task: Task) => void;
  upsertTasksBatch: (tasks: Task[]) => void;
  reorderTasksLocally: (orderedIds: string[]) => void;
  patchTask: (payload: TaskProgressPayload | unknown) => void;
  patchTasksBatch: (payloads: Array<TaskProgressPayload | unknown>) => void;
  setGlobalTaskStats: (stats: TaskStats | TaskStatsSnapshot | null) => void;
  toggleTaskExpanded: (id: string) => void;
  collapseTask: (id: string) => void;
  markCompletionFlash: (id: string) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  recalculateTaskStats: () => void;
}

/* ── Store ── */

export const useTaskDataStore = create<TaskDataStore>((set, get) => ({
  tasks: [],
  taskIds: [],
  taskById: {},
  taskIndexById: {},
  taskStats: { ...EMPTY_TASK_STATS },
  failureOptions: [],
  globalTaskStats: null,
  total: 0,
  page: 0,
  pageSize: 100,
  nextCursor: null,
  hasMore: false,
  filterOptions: {
    sources: [],
    failureCategories: [],
  },
  expandedTaskIds: [],
  completionFlashIds: [],
  loading: true,
  error: null,

  setTasks: (tasks) =>
    set((state) => {
      const ids = new Set(tasks.map((task) => task.id));

      // Delegate speed history pruning to dedicated store
      useSpeedHistoryStore.getState().pruneToIds(ids);

      // Cross-store: prune selectedIds in UI store
      pruneUISelectedIds(ids);

      return {
        ...taskCollections(tasks),
        total: tasks.length,
        page: 0,
        nextCursor: null,
        hasMore: false,
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
      };
    }),

  setTaskPage: (tasks, total, page, pageSize, append = false) =>
    set((state) => {
      const nextTasks = append ? mergePagedTasks(state.tasks, tasks) : tasks;
      const ids = new Set(nextTasks.map((task) => task.id));

      // Delegate speed history pruning to dedicated store
      useSpeedHistoryStore.getState().pruneToIds(ids);

      // Cross-store: prune selectedIds in UI store
      pruneUISelectedIds(ids);

      return {
        ...taskCollections(nextTasks),
        total,
        page,
        pageSize,
        nextCursor: null,
        hasMore: nextTasks.length < total,
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
      };
    }),

  setTaskCursorPage: (tasks, minimumTotal, nextCursor, filterOptions, append = false) =>
    set((state) => {
      const nextTasks = append ? mergePagedTasks(state.tasks, tasks) : tasks;
      const ids = new Set(nextTasks.map((task) => task.id));

      // Delegate speed history pruning to dedicated store
      useSpeedHistoryStore.getState().pruneToIds(ids);

      const knownTotal = Math.max(minimumTotal, nextTasks.length + (nextCursor ? 1 : 0));

      // Cross-store: prune selectedIds in UI store
      pruneUISelectedIds(ids);

      return {
        ...taskCollections(nextTasks),
        total: knownTotal,
        page: append ? state.page + 1 : 0,
        nextCursor,
        hasMore: Boolean(nextCursor),
        filterOptions:
          append && filterOptions.sources.length === 0 && filterOptions.failureCategories.length === 0
            ? state.filterOptions
            : filterOptions,
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        completionFlashIds: state.completionFlashIds.filter((id) => ids.has(id)),
      };
    }),

  upsertTask: (task) =>
    set((state) => {
      const existingIndex = state.taskIndexById[task.id];

      if (existingIndex === undefined) {
        // New task — prepend and rebuild arrays/maps
        const tasks = [task, ...state.tasks];
        return {
          ...taskCollections(tasks),
          total: Math.max(state.total, tasks.length),
        };
      }

      // Existing task — shallow-compare key fields to skip no-op updates
      const existing = state.tasks[existingIndex];
      const mergedFiles = (task.files?.length ?? 0) > 0 ? task.files : existing.files;
      if (
        existing.status === task.status &&
        existing.downloadedBytes === task.downloadedBytes &&
        existing.totalSize === task.totalSize &&
        existing.speedBps === task.speedBps &&
        existing.connectionCount === task.connectionCount &&
        existing.updatedAt === task.updatedAt &&
        existing.fileName === task.fileName &&
        existing.saveDir === task.saveDir &&
        existing.errorMessage === task.errorMessage &&
        mergedFiles === existing.files
      ) {
        return {};
      }

      // Surgical update: only replace the single entry, keep stable references
      const merged = { ...existing, ...task, files: mergedFiles };
      const tasks = [...state.tasks];
      tasks[existingIndex] = merged;
      const taskById = { ...state.taskById, [task.id]: merged };

      // E-3: Recompute failureOptions only when status/failureCategory/errorCode/errorMessage change,
      // avoiding recompute on every 250ms progress tick.
      const statusChanged =
        existing.status !== merged.status ||
        existing.failureCategory !== merged.failureCategory ||
        existing.errorCode !== merged.errorCode ||
        existing.errorMessage !== merged.errorMessage;

      return {
        tasks,
        taskById,
        // taskIds and taskIndexById are unchanged — keep existing references
        taskIds: state.taskIds,
        taskIndexById: state.taskIndexById,
        failureOptions: statusChanged ? computeFailureOptions(tasks) : state.failureOptions,
      };
    }),

  upsertTasksBatch: (incoming) =>
    set((state) => {
      if (incoming.length === 0) return {};
      const byId = new Map<string, Task>(Object.entries(state.taskById));
      let statusChangedAny = false;
      const newTasks: Task[] = [];
      for (const task of incoming) {
        const existingIndex = state.taskIndexById[task.id];
        if (existingIndex === undefined) {
          newTasks.push(task);
          byId.set(task.id, task);
        } else {
          const existing = state.tasks[existingIndex];
          const mergedFiles = (task.files?.length ?? 0) > 0 ? task.files : existing.files;
          const merged = { ...existing, ...task, files: mergedFiles };
          byId.set(task.id, merged);
          if (
            existing.status !== merged.status ||
            existing.failureCategory !== merged.failureCategory ||
            existing.errorCode !== merged.errorCode ||
            existing.errorMessage !== merged.errorMessage
          ) {
            statusChangedAny = true;
          }
        }
      }
      // Prepend new tasks (newest first), keep existing order for the rest.
      const tasks: Task[] =
        newTasks.length > 0
          ? [...newTasks, ...state.tasks.map((t) => byId.get(t.id) ?? t)]
          : state.tasks.map((t) => byId.get(t.id) ?? t);
      return {
        ...taskCollections(tasks),
        total: Math.max(state.total, tasks.length),
        failureOptions: statusChangedAny || newTasks.length > 0 ? computeFailureOptions(tasks) : state.failureOptions,
      };
    }),

  // UX-5: Optimistic local reorder of queued tasks. Replaces the affected
  // tasks in-place within the `tasks` array to match `orderedIds`, keeping
  // all non-affected tasks in their current positions. Used by handleReorder
  // to give immediate visual feedback before the backend confirms.
  reorderTasksLocally: (orderedIds) =>
    set((state) => {
      if (orderedIds.length === 0) return {};
      const affectedSet = new Set(orderedIds);
      const queue = [...orderedIds];
      const tasks = state.tasks.map((t) => {
        if (affectedSet.has(t.id)) {
          const nextId = queue.shift();
          return nextId ? (state.taskById[nextId] ?? t) : t;
        }
        return t;
      });
      return {
        tasks,
        taskIds: tasks.map((t) => t.id),
        taskIndexById: indexTasks(tasks),
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
    const speedSamples: Array<{ taskId: string; sample: { at: number; speedBps: number } }> = [];

    set((state) => {
      let tasks = state.tasks;
      let taskById = state.taskById;
      let changed = false;
      let statusChanged = false;

      // Incremental stats deltas
      let dActive = 0;
      let dQueued = 0;
      let dAttention = 0;
      let dPaused = 0;
      let dCompleted = 0;
      let dFailed = 0;
      let dTotalSpeed = 0;
      let dTotalDownloaded = 0;
      let dTotalBytes = 0;

      for (const payload of latestByTaskId.values()) {
        const index = state.taskIndexById[payload.taskId];
        if (index === undefined) continue;
        const old = state.tasks[index];
        const nextTask = applyProgressToTask(old, payload);
        // Zero-delta fast path: applyProgressToTask returns the same reference
        // when no field actually changed. Skip allocation and stats work.
        if (nextTask === old) continue;
        if (!changed) {
          tasks = [...state.tasks];
          taskById = { ...state.taskById };
          changed = true;
        }
        tasks[index] = nextTask;
        taskById[payload.taskId] = nextTask;
        speedSamples.push({
          taskId: payload.taskId,
          sample: { at: now, speedBps: nextTask.speedBps },
        });

        // Accumulate stats delta
        if (old.status !== nextTask.status) {
          statusChanged = true;
          for (const key of statusCounterKeys(old.status)) {
            if (key === "active") dActive -= 1;
            else if (key === "queued") dQueued -= 1;
            else if (key === "attention") dAttention -= 1;
            else if (key === "paused") dPaused -= 1;
            else if (key === "completed") dCompleted -= 1;
            else if (key === "failed") dFailed -= 1;
          }
          for (const key of statusCounterKeys(nextTask.status)) {
            if (key === "active") dActive += 1;
            else if (key === "queued") dQueued += 1;
            else if (key === "attention") dAttention += 1;
            else if (key === "paused") dPaused += 1;
            else if (key === "completed") dCompleted += 1;
            else if (key === "failed") dFailed += 1;
          }
        }
        dTotalSpeed += nextTask.speedBps - old.speedBps;
        dTotalDownloaded += nextTask.downloadedBytes - old.downloadedBytes;
        dTotalBytes += nextTask.totalSize - old.totalSize;
      }

      if (!changed) return {};

      const prevStats = state.taskStats;
      // Zero-delta fast path: if individual tasks changed but all aggregate
      // deltas cancelled out (e.g. one task sped up, another slowed down),
      // reuse the same stats reference so combined selectors like
      // `s.globalTaskStats ?? s.taskStats` don't trigger re-renders.
      const statsUnchanged =
        dActive === 0 &&
        dQueued === 0 &&
        dAttention === 0 &&
        dPaused === 0 &&
        dCompleted === 0 &&
        dFailed === 0 &&
        dTotalSpeed === 0 &&
        dTotalDownloaded === 0 &&
        dTotalBytes === 0;
      const nextStats: TaskStats = statsUnchanged
        ? prevStats
        : {
            all: prevStats.all,
            active: prevStats.active + dActive,
            queued: prevStats.queued + dQueued,
            attention: prevStats.attention + dAttention,
            paused: prevStats.paused + dPaused,
            completed: prevStats.completed + dCompleted,
            failed: prevStats.failed + dFailed,
            totalSpeed: Math.max(0, prevStats.totalSpeed + dTotalSpeed),
            totalDownloaded: Math.max(0, prevStats.totalDownloaded + dTotalDownloaded),
            totalBytes: Math.max(0, prevStats.totalBytes + dTotalBytes),
            featuredTaskId: prevStats.featuredTaskId,
          };

      return {
        tasks,
        taskById,
        taskStats: nextStats,
        taskIds: state.taskIds,
        taskIndexById: state.taskIndexById,
        // E-3: Recompute only on status change, avoiding failureOptions recompute on every 250ms progress tick
        failureOptions: statusChanged ? computeFailureOptions(tasks) : state.failureOptions,
      };
    });

    // Delegate speed history to dedicated store (avoids mutating main store on every frame)
    if (speedSamples.length > 0) {
      useSpeedHistoryStore.getState().appendBatch(speedSamples);
    }
  },

  setGlobalTaskStats: (stats) =>
    set({
      globalTaskStats: stats ? normalizeTaskStatsSnapshot(stats) : null,
    }),

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

  recalculateTaskStats: () => {
    const tasks = get().tasks;
    const newStats = calculateTaskStats(tasks);
    const current = get().taskStats;
    // Shallow equality check to avoid unnecessary re-renders
    if (
      newStats.all === current.all &&
      newStats.active === current.active &&
      newStats.queued === current.queued &&
      newStats.attention === current.attention &&
      newStats.paused === current.paused &&
      newStats.completed === current.completed &&
      newStats.failed === current.failed &&
      newStats.totalSpeed === current.totalSpeed &&
      newStats.totalDownloaded === current.totalDownloaded &&
      newStats.totalBytes === current.totalBytes &&
      newStats.featuredTaskId === current.featuredTaskId
    ) {
      return;
    }
    set({ taskStats: newStats });
  },
}));

/* ── Cross-store helper ── */

/** Prune selectedIds in the UI store when task set changes. */
function pruneUISelectedIds(activeTaskIds: Set<string>) {
  const uiState = useTaskUIStore.getState();
  const filtered = uiState.selectedIds.filter((id: string) => activeTaskIds.has(id));
  if (filtered.length !== uiState.selectedIds.length) {
    useTaskUIStore.setState({ selectedIds: filtered });
  }
}

/* ── Query input helpers (read from both stores) ── */

export function taskPageInput(page = useTaskDataStore.getState().page): ListTasksInput {
  const data = useTaskDataStore.getState();
  const ui = useTaskUIStore.getState();
  return buildTaskPageInput(
    {
      nav: ui.nav,
      search: ui.search,
      sortKey: ui.sortKey,
      sortDirection: ui.sortDirection,
      filters: ui.filters,
      page,
      pageSize: data.pageSize,
    },
    page,
  );
}

export function taskCursorInput(cursor: string | null = null): ListTasksCursorInput {
  const data = useTaskDataStore.getState();
  const ui = useTaskUIStore.getState();
  return buildTaskCursorInput(
    {
      nav: ui.nav,
      search: ui.search,
      sortKey: ui.sortKey,
      sortDirection: ui.sortDirection,
      filters: ui.filters,
      page: data.page,
      pageSize: data.pageSize,
    },
    cursor,
  );
}
