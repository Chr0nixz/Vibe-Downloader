import { useEffect, useRef } from "react";
import i18n from "@/i18n";

import { listTasks, onQueueChanged, onTaskProgress } from "@/lib/tauri";
import { mergeTasksFromServer, useTaskStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

/** Subscribe once to backend progress/queue events for the app lifetime. */
export function useTaskEvents() {
  const notifiedStatuses = useRef(new Set<string>());

  useEffect(() => {
    let cancelled = false;
    let unlistenProgress: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;

    function notifyTaskStatusChanges(previous: Task[], next: Task[]) {
      const previousById = new Map(previous.map((task) => [task.id, task]));
      const addToast = useToastStore.getState().addToast;

      for (const task of next) {
        const previousTask = previousById.get(task.id);
        if (!previousTask || previousTask.status === task.status) continue;
        if (
          task.status !== "completed" &&
          task.status !== "failed" &&
          task.status !== "needs_attention"
        ) {
          notifiedStatuses.current.delete(`${task.id}:failed`);
          notifiedStatuses.current.delete(`${task.id}:needs_attention`);
          notifiedStatuses.current.delete(`${task.id}:completed`);
          continue;
        }
        const notificationKey = `${task.id}:${task.status}`;
        if (notifiedStatuses.current.has(notificationKey)) continue;

        if (task.status === "completed") {
          notifiedStatuses.current.add(notificationKey);
          addToast({
            tone: "success",
            title: i18n.t("toast.taskCompleted", { name: task.fileName }),
          });
        }

        if (task.status === "failed" || task.status === "needs_attention") {
          notifiedStatuses.current.add(notificationKey);
          addToast({
            tone: "error",
            title: i18n.t("toast.taskFailed", { name: task.fileName }),
            description: task.errorMessage || task.healthSummary || undefined,
          });
        }
      }
    }

    void (async () => {
      unlistenProgress = await onTaskProgress((payload) => {
        if (!cancelled) {
          const previous = useTaskStore.getState().tasks;
          useTaskStore.getState().patchTask(payload);
          notifyTaskStatusChanges(previous, useTaskStore.getState().tasks);
        }
      });
      if (cancelled) {
        unlistenProgress();
        return;
      }

      unlistenQueue = await onQueueChanged(async () => {
        if (cancelled) return;
        try {
          const previous = useTaskStore.getState().tasks;
          const fresh = await listTasks();
          if (cancelled) return;
          const merged = mergeTasksFromServer(previous, fresh);
          useTaskStore.getState().setTasks(merged);
          notifyTaskStatusChanges(previous, merged);
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
