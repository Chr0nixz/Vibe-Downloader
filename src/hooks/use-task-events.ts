import { useEffect, useRef } from "react";
import i18n from "@/i18n";

import { createLogger } from "@/lib/logger";
import {
  isTauriRuntime,
  listTasksCursor,
  onQueueChanged,
  onTaskProgress,
  onTaskUpdated,
} from "@/lib/tauri";
import { mergeTasksFromServer, taskCursorInput, useTaskStore } from "@/stores/task-store";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";
import { errorMessage } from "@/lib/errors";
import type { Task } from "@/types/task";

const log = createLogger("task-events");
const MAX_NOTIFIED_STATUS_KEYS = 600;

export function rememberStatusNotification(
  notifiedStatuses: Set<string>,
  notificationKey: string,
  maxKeys = MAX_NOTIFIED_STATUS_KEYS,
): boolean {
  if (notifiedStatuses.has(notificationKey)) return false;
  while (notifiedStatuses.size >= maxKeys) {
    const oldest = notifiedStatuses.values().next().value;
    if (!oldest) break;
    notifiedStatuses.delete(oldest);
  }
  notifiedStatuses.add(notificationKey);
  return true;
}

function clearTaskStatusNotifications(notifiedStatuses: Set<string>, taskId: string) {
  notifiedStatuses.delete(`${taskId}:failed`);
  notifiedStatuses.delete(`${taskId}:needs_attention`);
  notifiedStatuses.delete(`${taskId}:completed`);
}

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
          clearTaskStatusNotifications(notifiedStatuses.current, task.id);
          continue;
        }
        const notificationKey = `${task.id}:${task.status}`;
        if (!rememberStatusNotification(notifiedStatuses.current, notificationKey)) continue;

        if (task.status === "completed") {
          useTaskStore.getState().markCompletionFlash(task.id);
          addToast({
            tone: "success",
            title: i18n.t("toast.taskCompleted", { name: task.fileName }),
          });
          void sendCompletionNotification(task);
        }

        if (task.status === "failed" || task.status === "needs_attention") {
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
              const page = await listTasksCursor(taskCursorInput(null));
              const fresh = page.items;
              if (cancelled) return;
              const merged = mergeTasksFromServer(previous, fresh);
              useTaskStore.getState().setTaskCursorPage(
                merged,
                page.totalEstimate,
                page.nextCursor,
                page.filterOptions,
              );
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
