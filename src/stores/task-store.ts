import { create } from "zustand";

import type { Task } from "@/types/task";
import { parseByteCount } from "@/types/task";
import {
  normalizeTaskProgressPayload,
  type TaskProgressPayload,
} from "@/types/task-progress";

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
  loading: boolean;
  error: string | null;
  setTasks: (tasks: Task[]) => void;
  upsertTask: (task: Task) => void;
  patchTask: (payload: TaskProgressPayload | unknown) => void;
  selectTask: (id: string | null) => void;
  setNav: (nav: NavFilter) => void;
  setSearch: (search: string) => void;
  setDetailOpen: (open: boolean) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],
  selectedId: null,
  nav: "all",
  search: "",
  detailOpen: false,
  loading: true,
  error: null,
  setTasks: (tasks) => set({ tasks }),
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

    set((state) => ({
      tasks: state.tasks.map((task) =>
        task.id === payload.taskId
          ? {
              ...task,
              downloadedBytes: parseByteCount(payload.downloadedBytes),
              totalSize: parseByteCount(payload.totalSize),
              speedBps: parseByteCount(payload.speedBps),
              connectionCount: payload.connectionCount,
              status: payload.status,
            }
          : task,
      ),
    }));
  },
  selectTask: (id) => set({ selectedId: id }),
  setNav: (nav) => set({ nav }),
  setSearch: (search) => set({ search }),
  setDetailOpen: (open) => set({ detailOpen: open }),
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
