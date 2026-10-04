import { useEffect, useRef } from "react";
import type { DesktopStatusUpdate, TaskStatsSnapshot } from "@/generated/bindings";
import i18n from "@/i18n";
import { localizedErrorMessage, localizedMessage } from "@/lib/errors";
import { bumpListQueryEpoch, isCurrentListQueryEpoch } from "@/lib/list-query-epoch";
import { createLogger } from "@/lib/logger";
import { sendSystemNotification } from "@/lib/system-notification";
import {
  getTaskStats,
  listTasksByIds,
  listTasksCursor,
  onDesktopStatus,
  onQueueChanged,
  onTaskProgress,
  onTaskUpdated,
  updateDesktopStatus,
} from "@/lib/tauri";
import { formatSpeed } from "@/lib/utils";
import {
  mergeTasksFromServer,
  normalizeTaskStatsSnapshot,
  taskCursorInput,
  useTaskDataStore,
} from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

const log = createLogger("task-events");
const MAX_NOTIFIED_STATUS_KEYS = 600;
const QUEUE_INCREMENTAL_ID_LIMIT = 50;

export function buildDesktopStatusUpdate(stats: TaskStatsSnapshot): DesktopStatusUpdate {
  const active = Number(stats.active) || 0;
  const queued = Number(stats.queued) || 0;
  const attention = Number(stats.attention) || 0;
  const failed = Number(stats.failed) || 0;
  const downloaded = Number(stats.totalDownloaded) || 0;
  const total = Number(stats.totalBytes) || 0;
  const progress = active > 0 && total > 0 ? Math.round(Math.min(100, Math.max(0, (downloaded / total) * 100))) : null;
  const parts = [
    active > 0
      ? i18n.t("statusBar.downloadingAt", { count: active, speed: formatSpeed(Number(stats.totalSpeed) || 0) })
      : i18n.t("trayMenu.status"),
  ];
  if (queued > 0) parts.push(i18n.t("statusBar.queuedOnly", { count: queued }));
  if (attention > 0) parts.push(i18n.t("statusBar.attentionCount", { count: attention }));
  if (failed > 0) parts.push(i18n.t("statusBar.failedCount", { count: failed }));
  return {
    tooltip: parts.join(" · "),
    progress,
    hasError: attention > 0 || failed > 0,
  };
}

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

export type FailureNotificationCounts = { failed: number; attention: number };

/** Merge transitions that arrive during the short native-notification window. */
export function mergeFailureNotificationCounts(
  current: FailureNotificationCounts,
  failed: number,
  attention: number,
): FailureNotificationCounts {
  return {
    failed: current.failed + Math.max(0, failed),
    attention: current.attention + Math.max(0, attention),
  };
}

function clearTaskStatusNotifications(notifiedStatuses: Set<string>, taskId: string) {
  notifiedStatuses.delete(`${taskId}:failed`);
  notifiedStatuses.delete(`${taskId}:needs_attention`);
  notifiedStatuses.delete(`${taskId}:completed`);
}

/** ARC-09: debounce window accumulator for queue-changed payloads. */
export type QueueChangedAccumulator = {
  ids: Set<string>;
  fullRefresh: boolean;
};

export function createQueueChangedAccumulator(): QueueChangedAccumulator {
  return { ids: new Set(), fullRefresh: false };
}

export type QueueChangedPayloadLike = {
  changed_task_ids?: string[] | null;
} | null;

/** Merge one queue-changed payload into the debounce accumulator. */
export function accumulateQueueChanged(state: QueueChangedAccumulator, payload: QueueChangedPayloadLike): void {
  const ids = payload?.changed_task_ids ?? null;
  if (ids == null) {
    state.fullRefresh = true;
    state.ids.clear();
    return;
  }
  if (state.fullRefresh) return;
  for (const id of ids) {
    state.ids.add(id);
  }
}

export type QueueFlushPlan = { kind: "noop" } | { kind: "incremental"; ids: string[] } | { kind: "full" };

/** Decide flush strategy and clear the accumulator. */
export function takeQueueFlushPlan(
  state: QueueChangedAccumulator,
  incrementalLimit = QUEUE_INCREMENTAL_ID_LIMIT,
): QueueFlushPlan {
  if (state.fullRefresh) {
    state.fullRefresh = false;
    state.ids.clear();
    return { kind: "full" };
  }
  if (state.ids.size === 0) return { kind: "noop" };
  if (state.ids.size > incrementalLimit) {
    state.ids.clear();
    return { kind: "full" };
  }
  const ids = [...state.ids];
  state.ids.clear();
  return { kind: "incremental", ids };
}

interface UseTaskEventsOptions {
  notify?: boolean;
}

/** Subscribe once to backend progress/queue events for the app lifetime. */
export function useTaskEvents(options: UseTaskEventsOptions = {}) {
  const notify = options.notify ?? true;
  const notifiedStatuses = useRef(new Set<string>());

  // Prune notifiedStatuses when tasks are removed from the store.
  // Use the stable `taskIds` array (only changes on add/remove) instead of
  // `tasks.map(...)` which returns a new array on every progress tick.
  const taskIds = useTaskDataStore((s) => s.taskIds);
  useEffect(() => {
    const activeIds = new Set(taskIds);
    for (const key of notifiedStatuses.current) {
      const colonIndex = key.lastIndexOf(":");
      if (colonIndex === -1) continue;
      const taskId = key.slice(0, colonIndex);
      if (!activeIds.has(taskId)) {
        notifiedStatuses.current.delete(key);
      }
    }
  }, [taskIds]);

  useEffect(() => {
    let cancelled = false;
    let unlistenProgress: (() => void) | undefined;
    let unlistenTaskUpdated: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;
    let unlistenDesktopStatus: (() => void) | undefined;
    let queueRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let statsRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let statsRefreshInFlight = false;
    let recalculateStatsTimer: ReturnType<typeof setTimeout> | undefined;
    let progressFrame: number | undefined;
    let progressFallbackTimer: ReturnType<typeof setTimeout> | undefined;
    let failureNotificationTimer: ReturnType<typeof setTimeout> | undefined;
    let pendingFailureCounts: FailureNotificationCounts = { failed: 0, attention: 0 };
    let pendingProgressPayloads: unknown[] = [];
    const pendingQueue = createQueueChangedAccumulator();

    function notifyTaskStatusTransitions(
      transitions: Array<{ taskId: string; previousStatus: Task["status"]; task: Task }>,
    ) {
      if (!notify || transitions.length === 0) return;
      const addToast = useToastStore.getState().addToast;
      let failedTransitions = 0;
      let attentionTransitions = 0;

      for (const { taskId, previousStatus, task } of transitions) {
        if (previousStatus === task.status) continue;
        if (task.status !== "completed" && task.status !== "failed" && task.status !== "needs_attention") {
          clearTaskStatusNotifications(notifiedStatuses.current, taskId);
          continue;
        }
        const notificationKey = `${task.id}:${task.status}`;
        if (!rememberStatusNotification(notifiedStatuses.current, notificationKey)) continue;

        if (task.status === "completed") {
          useTaskDataStore.getState().markCompletionFlash(task.id);
          addToast({
            tone: "success",
            title: i18n.t("toast.taskCompleted", { name: task.fileName }),
          });
          void sendCompletionNotification(task);
        }

        if (task.status === "failed" || task.status === "needs_attention") {
          if (task.status === "failed") failedTransitions += 1;
          else attentionTransitions += 1;
          addToast({
            tone: "error",
            title: i18n.t("toast.taskFailed", { name: task.fileName }),
            description: task.errorMessage
              ? localizedErrorMessage(task.errorMessage, i18n.t)
              : localizedMessage(task.healthSummary, i18n.t),
          });
        }
      }

      if (failedTransitions > 0 || attentionTransitions > 0) {
        pendingFailureCounts = mergeFailureNotificationCounts(
          pendingFailureCounts,
          failedTransitions,
          attentionTransitions,
        );
        if (!failureNotificationTimer) {
          failureNotificationTimer = setTimeout(() => {
            failureNotificationTimer = undefined;
            const counts = pendingFailureCounts;
            pendingFailureCounts = { failed: 0, attention: 0 };
            void sendFailureNotification(counts.failed, counts.attention);
          }, 700);
        }
      }
    }

    function notifyTaskStatusChanges(previous: Task[], next: Task[]) {
      if (!notify) return;
      const previousById = new Map(previous.map((task) => [task.id, task]));
      const transitions = next.flatMap((task) => {
        const previousTask = previousById.get(task.id);
        if (!previousTask || previousTask.status === task.status) return [];
        return [{ taskId: task.id, previousStatus: previousTask.status, task }];
      });
      notifyTaskStatusTransitions(transitions);
    }

    function scheduleStatsRefresh(delay = 250) {
      if (statsRefreshTimer) clearTimeout(statsRefreshTimer);
      statsRefreshTimer = setTimeout(() => {
        statsRefreshTimer = undefined;
        if (statsRefreshInFlight) {
          scheduleStatsRefresh(250);
          return;
        }
        statsRefreshInFlight = true;
        void getTaskStats()
          .then((stats) => {
            if (!cancelled) {
              useTaskDataStore.getState().setGlobalTaskStats(normalizeTaskStatsSnapshot(stats));
              void updateDesktopStatus(buildDesktopStatusUpdate(stats)).catch((error) => {
                log.debug("desktop status update failed", error);
              });
            }
          })
          .catch((error) => {
            log.warn("task stats refresh failed", error);
          })
          .finally(() => {
            statsRefreshInFlight = false;
          });
      }, delay);
    }

    function scheduleRecalculateStats(delay: number) {
      if (recalculateStatsTimer) clearTimeout(recalculateStatsTimer);
      recalculateStatsTimer = setTimeout(() => {
        recalculateStatsTimer = undefined;
        useTaskDataStore.getState().recalculateTaskStats();
      }, delay);
    }

    function flushProgressBatch() {
      if (progressFrame !== undefined) {
        cancelAnimationFrame(progressFrame);
        progressFrame = undefined;
      }
      if (progressFallbackTimer) {
        clearTimeout(progressFallbackTimer);
        progressFallbackTimer = undefined;
      }
      if (pendingProgressPayloads.length === 0) return;

      const payloads = pendingProgressPayloads;
      pendingProgressPayloads = [];
      // PERF-03: toast work tracks statusTransitions from the patch, not the full loaded list.
      const { statusTransitions } = useTaskDataStore.getState().patchTasksBatch(payloads);
      notifyTaskStatusTransitions(statusTransitions);
      scheduleRecalculateStats(250);
    }

    function scheduleProgressFlush() {
      if (progressFrame !== undefined || progressFallbackTimer) return;

      if (typeof requestAnimationFrame === "function") {
        progressFrame = requestAnimationFrame(() => {
          progressFrame = undefined;
          if (progressFallbackTimer) {
            clearTimeout(progressFallbackTimer);
            progressFallbackTimer = undefined;
          }
          flushProgressBatch();
        });
        progressFallbackTimer = setTimeout(() => {
          if (progressFrame !== undefined) {
            cancelAnimationFrame(progressFrame);
            progressFrame = undefined;
          }
          flushProgressBatch();
        }, 80);
        return;
      }

      progressFallbackTimer = setTimeout(() => {
        progressFallbackTimer = undefined;
        flushProgressBatch();
      }, 16);
    }

    void (async () => {
      // Subscribe to all three event streams in parallel — they have no
      // dependency on each other, so sequential awaits only add IPC round-trip
      // latency before the queue listener is registered.
      const results = await Promise.allSettled([
        onTaskProgress((payload) => {
          if (!cancelled) {
            pendingProgressPayloads.push(payload);
            scheduleProgressFlush();
          }
        }),
        onTaskUpdated((task) => {
          if (cancelled) return;
          flushProgressBatch();
          const previous = useTaskDataStore.getState().tasks;
          useTaskDataStore.getState().upsertTask(task);
          notifyTaskStatusChanges(previous, useTaskDataStore.getState().tasks);
          scheduleRecalculateStats(150);
        }),
        onQueueChanged((payload) => {
          if (cancelled) return;
          flushProgressBatch();
          accumulateQueueChanged(pendingQueue, payload);
          if (queueRefreshTimer) clearTimeout(queueRefreshTimer);
          queueRefreshTimer = setTimeout(() => {
            queueRefreshTimer = undefined;
            void (async () => {
              const plan = takeQueueFlushPlan(pendingQueue);
              if (plan.kind === "noop") return;
              try {
                if (plan.kind === "incremental") {
                  const changed = await listTasksByIds(plan.ids);
                  if (cancelled) return;
                  const previous = useTaskDataStore.getState().tasks;
                  useTaskDataStore.getState().upsertTasksBatch(changed);
                  notifyTaskStatusChanges(previous, useTaskDataStore.getState().tasks);
                  scheduleRecalculateStats(150);
                  scheduleStatsRefresh(150);
                  return;
                }

                // ARC-07: full refresh shares the list query epoch.
                const epoch = bumpListQueryEpoch();
                const previous = useTaskDataStore.getState().tasks;
                const page = await listTasksCursor(taskCursorInput(null));
                if (cancelled || !isCurrentListQueryEpoch(epoch)) return;
                const fresh = page.items;
                const merged = mergeTasksFromServer(previous, fresh);
                useTaskDataStore
                  .getState()
                  .setTaskCursorPage(merged, page.minimumTotal, page.nextCursor, page.filterOptions);
                notifyTaskStatusChanges(previous, merged);
                scheduleRecalculateStats(150);
                scheduleStatsRefresh(150);
              } catch (error) {
                log.warn("queue refresh failed", error);
              }
            })();
          }, 100);
        }),
        notify
          ? onDesktopStatus((stats) => {
              if (!cancelled) {
                void updateDesktopStatus(buildDesktopStatusUpdate(stats)).catch((error) => {
                  log.debug("desktop status update failed", error);
                });
              }
            })
          : Promise.resolve(() => {}),
      ]);

      if (results[0].status === "fulfilled") unlistenProgress = results[0].value;
      else log.warn("task progress listener registration failed", results[0].reason);
      if (results[1].status === "fulfilled") unlistenTaskUpdated = results[1].value;
      else log.warn("task updated listener registration failed", results[1].reason);
      if (results[2].status === "fulfilled") unlistenQueue = results[2].value;
      else log.warn("queue changed listener registration failed", results[2].reason);
      if (results[3].status === "fulfilled") unlistenDesktopStatus = results[3].value;
      else log.warn("desktop status listener registration failed", results[3].reason);

      if (cancelled) {
        unlistenProgress?.();
        unlistenTaskUpdated?.();
        unlistenQueue?.();
        unlistenDesktopStatus?.();
      }
    })();

    scheduleStatsRefresh(0);

    return () => {
      cancelled = true;
      if (queueRefreshTimer) clearTimeout(queueRefreshTimer);
      if (statsRefreshTimer) clearTimeout(statsRefreshTimer);
      if (recalculateStatsTimer) clearTimeout(recalculateStatsTimer);
      if (progressFrame !== undefined) cancelAnimationFrame(progressFrame);
      if (progressFallbackTimer) clearTimeout(progressFallbackTimer);
      if (failureNotificationTimer) clearTimeout(failureNotificationTimer);
      unlistenProgress?.();
      unlistenTaskUpdated?.();
      unlistenQueue?.();
      unlistenDesktopStatus?.();
    };
  }, [notify]);
}

async function sendCompletionNotification(task: Task) {
  await sendSystemNotification(i18n.t("toast.taskCompleted", { name: task.fileName }), task.saveDir);
}

async function sendFailureNotification(failed: number, attention: number) {
  const parts: string[] = [];
  if (attention > 0) parts.push(i18n.t("statusBar.attentionCount", { count: attention }));
  if (failed > 0) parts.push(i18n.t("statusBar.failedCount", { count: failed }));
  if (parts.length === 0) return;
  await sendSystemNotification(i18n.t("trayMenu.title"), parts.join(" · "));
}
