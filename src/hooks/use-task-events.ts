import { useEffect, useRef } from "react";
import i18n from "@/i18n";

import { createLogger } from "@/lib/logger";
import {
  isTauriRuntime,
  listTasksPage,
  onQueueChanged,
  onTaskProgress,
  onTaskUpdated,
} from "@/lib/tauri";

const log = createLogger("task-events");
import { mergeTasksFromServer, taskPageInput, useTaskStore } from "@/stores/task-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";
import { errorMessage } from "@/lib/errors";
import type { Task } from "@/types/task";

interface UseTaskEventsOptions {
  notify?: boolean;
}

/** Subscribe once to backend progress/queue events for the app lifetime. */
export function useTaskEvents(options: UseTaskEventsOptions = {}) {
  const notify = options.notify ?? true;
  const notifiedStatuses = useRef(new Set<string>());

  useEffect(() => {
    let cancelled = false;
    let unlistenProgress: (() => void) | undefined;
    let unlistenTaskUpdated: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;
    let queueRefreshTimer: ReturnType<typeof setTimeout> | undefined;

    function notifyTaskStatusChanges(previous: Task[], next: Task[]) {
      if (!notify) return;
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
          void sendCompletionNotification(task);
        }

        if (task.status === "failed" || task.status === "needs_attention") {
          notifiedStatuses.current.add(notificationKey);
          addToast({
            tone: "error",
            title: i18n.t("toast.taskFailed", { name: task.fileName }),
            description: task.errorMessage ? errorMessage(task.errorMessage) : task.healthSummary || undefined,
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

      unlistenTaskUpdated = await onTaskUpdated((task) => {
        if (cancelled) return;
        const previous = useTaskStore.getState().tasks;
        useTaskStore.getState().upsertTask(task);
        notifyTaskStatusChanges(previous, useTaskStore.getState().tasks);
      });
      if (cancelled) {
        unlistenTaskUpdated();
        return;
      }

      unlistenQueue = await onQueueChanged(async () => {
        if (cancelled) return;
        if (queueRefreshTimer) clearTimeout(queueRefreshTimer);
        queueRefreshTimer = setTimeout(() => {
          void (async () => {
            try {
              const previous = useTaskStore.getState().tasks;
              const page = await listTasksPage(taskPageInput(0));
              const fresh = page.items;
              if (cancelled) return;
              const merged = mergeTasksFromServer(previous, fresh);
              useTaskStore.getState().setTaskPage(merged, page.total, page.page, page.pageSize);
              notifyTaskStatusChanges(previous, merged);
            } catch (error) {
              log.warn("queue refresh failed", error);
            }
          })();
        }, 100);
      });
      if (cancelled) {
        unlistenQueue?.();
      }
    })();

    return () => {
      cancelled = true;
      if (queueRefreshTimer) clearTimeout(queueRefreshTimer);
      unlistenProgress?.();
      unlistenTaskUpdated?.();
      unlistenQueue?.();
    };
  }, [notify]);
}

async function sendCompletionNotification(task: Task) {
  if (!isTauriRuntime()) return;
  if (!useSettingsStore.getState().settings?.systemNotifications) return;

  try {
    const {
      isPermissionGranted,
      requestPermission,
      sendNotification,
    } = await import("@tauri-apps/plugin-notification");
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    if (!granted) return;
    sendNotification({
      title: i18n.t("toast.taskCompleted", { name: task.fileName }),
      body: task.saveDir,
    });
  } catch (error) {
    log.warn("system notification failed", error);
  }
}
