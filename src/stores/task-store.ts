import { create } from "zustand";

import type { Task } from "@/types/task";
import { parseByteCount } from "@/types/task";
import type { TaskProgressPayload } from "@/types/task-progress";

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
  patchTask: (payload: TaskProgressPayload) => void;
  selectTask: (id: string | null) => void;
  setNav: (nav: NavFilter) => void;
  setSearch: (search: string) => void;
  setDetailOpen: (open: boolean) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useTaskStore = create<TaskStore>((set, get) => ({
  tasks: [],
  selectedId: null,
  nav: "all",
  search: "",
  detailOpen: true,
  loading: true,
  error: null,
  setTasks: (tasks) => set({ tasks }),
  patchTask: (payload) => {
    set({
      tasks: get().tasks.map((task) =>
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
    });
  },
  selectTask: (id) => set({ selectedId: id }),
  setNav: (nav) => set({ nav }),
  setSearch: (search) => set({ search }),
  setDetailOpen: (open) => set({ detailOpen: open }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
}));

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
