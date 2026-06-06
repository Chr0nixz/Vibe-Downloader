import { useEffect } from "react";

import { listTasks } from "@/lib/tauri";
import { useTaskStore } from "@/stores/task-store";

const ACTIVE_POLL_MS = 500;

function hasActiveDownloads(tasks: ReturnType<typeof useTaskStore.getState>["tasks"]) {
  return tasks.some(
    (task) =>
      (task.status === "downloading" || task.status === "retrying") &&
      (task.totalSize <= 0 || task.downloadedBytes < task.totalSize),
  );
}

/** Poll task list while downloads run — mirrors chunk panel DB sync. */
export function useActiveDownloadSync() {
  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    async function syncFromServer() {
      if (!hasActiveDownloads(useTaskStore.getState().tasks)) return;
      try {
        const fresh = await listTasks();
        if (!cancelled) useTaskStore.getState().setTasks(fresh);
      } catch {
        /* ignore transient refresh errors */
      }
    }

    function startPolling() {
      if (intervalId) return;
      void syncFromServer();
      intervalId = setInterval(() => {
        void syncFromServer();
      }, ACTIVE_POLL_MS);
    }

    function stopPolling() {
      if (!intervalId) return;
      clearInterval(intervalId);
      intervalId = undefined;
    }

    if (hasActiveDownloads(useTaskStore.getState().tasks)) {
      startPolling();
    }

    const unsubscribe = useTaskStore.subscribe((state, prevState) => {
      const active = hasActiveDownloads(state.tasks);
      const wasActive = hasActiveDownloads(prevState.tasks);
      if (active && !wasActive) startPolling();
      else if (!active && wasActive) stopPolling();
    });

    return () => {
      cancelled = true;
      stopPolling();
      unsubscribe();
    };
  }, []);
}
