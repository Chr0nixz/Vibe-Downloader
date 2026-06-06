import { create } from "zustand";

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

interface TaskStore {
  tasks: Task[];
  selectedId: string | null;
  nav: NavFilter;
  search: string;
  detailOpen: boolean;
  expandedTaskIds: string[];
  speedHistoryByTaskId: Record<string, SpeedSample[]>;
  loading: boolean;
  error: string | null;
  setTasks: (tasks: Task[]) => void;
  upsertTask: (task: Task) => void;
  patchTask: (payload: TaskProgressPayload | unknown) => void;
  selectTask: (id: string | null) => void;
  setNav: (nav: NavFilter) => void;
  setSearch: (search: string) => void;
  setDetailOpen: (open: boolean) => void;
  toggleTaskExpanded: (id: string) => void;
  collapseTask: (id: string) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],
  selectedId: null,
  nav: "all",
  search: "",
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
        expandedTaskIds: state.expandedTaskIds.filter((id) => ids.has(id)),
        speedHistoryByTaskId,
      };
    }),
  upsertTask: (task) =>
    set((state) => {
      const index = state.tasks.findIndex((entry) => entry.id === task.id);
      if (index < 0) return { tasks: [task, ...state.tasks] };
      const tasks = [...state.tasks];
      tasks[index] = { ...tasks[index], ...task };
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
  setNav: (nav) => set({ nav }),
  setSearch: (search) => set({ search }),
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
): Task[] {
  const query = search.trim().toLowerCase();

  return tasks.filter((task) => {
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
      task.sourceHost.toLowerCase().includes(query) ||
      task.url.toLowerCase().includes(query)
    );
  });
}
