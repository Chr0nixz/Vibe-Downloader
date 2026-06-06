import { useEffect } from "react";

import { listTasks, onQueueChanged, onTaskProgress } from "@/lib/tauri";
import { mergeTasksFromServer, useTaskStore } from "@/stores/task-store";

/** Subscribe once to backend progress/queue events for the app lifetime. */
export function useTaskEvents() {
  useEffect(() => {
    let cancelled = false;
    let unlistenProgress: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;

    void (async () => {
      unlistenProgress = await onTaskProgress((payload) => {
        if (!cancelled) useTaskStore.getState().patchTask(payload);
      });
      if (cancelled) {
        unlistenProgress();
        return;
      }

      unlistenQueue = await onQueueChanged(async () => {
        if (cancelled) return;
        try {
          const fresh = await listTasks();
          if (cancelled) return;
          useTaskStore
            .getState()
            .setTasks(mergeTasksFromServer(useTaskStore.getState().tasks, fresh));
        } catch {
          /* ignore refresh errors */
        }
      });
      if (cancelled) {
        unlistenQueue?.();
      }
    })();

    return () => {
      cancelled = true;
      unlistenProgress?.();
      unlistenQueue?.();
    };
  }, []);
}
