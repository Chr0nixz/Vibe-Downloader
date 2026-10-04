import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CloseDownloadDialog } from "@/components/shell/CloseDownloadDialog";
import { CommandBar } from "@/components/shell/CommandBar";
import { navFilterForDigit } from "@/components/shell/nav-shortcuts";
import type { AttentionDialogRequest } from "@/components/shell/ResolveAttentionDialog";
import { ShutdownOverlay } from "@/components/shell/ShutdownOverlay";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import { isGlobalPasteShortcut, isOverlayKey } from "@/components/shell/shell-keys";
import { TitleBar } from "@/components/shell/TitleBar";
import type { ReorderAction } from "@/components/tasks/TaskContextMenu";
import { TaskList } from "@/components/tasks/TaskList";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { LiveRegion } from "@/components/ui/live-region";
import { ToastViewport } from "@/components/ui/toast";
import type {
  CloseRequestAction,
  CloseRequestPayload,
  CompletionAction,
  CompletionActionRequestedPayload,
  RecoveryAction,
  ResolveTaskAttentionInput,
  TaskPriority,
} from "@/generated/bindings";
import { useClipboardLinkPrompt } from "@/hooks/use-clipboard-link-prompt";
import { useFileDropMonitor } from "@/hooks/use-file-drop-monitor";
import { useSelectAllMatching } from "@/hooks/use-select-all-matching";
import { useChromeLayout } from "@/hooks/use-shell-layout";
import { useTaskEvents } from "@/hooks/use-task-events";
import type { TranslationKey } from "@/i18n";
import { writeClipboardText } from "@/lib/clipboard-write";
import { localizedErrorMessage } from "@/lib/errors";
import { bumpListQueryEpoch, isCurrentListQueryEpoch } from "@/lib/list-query-epoch";
import { createLogger } from "@/lib/logger";
import { isModalFocusActive } from "@/lib/modal-focus";
import { getPlatform, type Platform, trafficLightsInsetPx } from "@/lib/platform";
import { writeSettingsRecoveryReturn } from "@/lib/settings-recovery-return";
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";

const log = createLogger("app-shell");

import {
  allowedTransferActions,
  hasInlineRecovery,
  pauseWouldDiscardProgress,
  primaryRecoveryAction,
  torrentFileSelectionRequired,
} from "@/components/tasks/row-recovery";
import { isSupportedLocalFile, resolveLocalFile } from "@/lib/local-file";
import {
  bulkDeleteTasks,
  bulkTaskAction,
  bulkTaskActionGlobal,
  deleteTask,
  finishLiveRecording,
  getSettings,
  listTasksCursor,
  onBrowserHandoffAuthorizationRequired,
  onCloseRequested,
  onCompletionActionRequested,
  onSettingsChanged,
  onTrayNewDownloadRequested,
  onTraySettingsRequested,
  openDirectoryPicker,
  openTaskFile,
  openTaskFolder,
  pauseTask,
  queryDiskSpace,
  recheckTask,
  redownloadTask,
  reorderQueuedTasks,
  requestLockScreen,
  requestSystemHibernate,
  requestSystemShutdown,
  requestSystemSleep,
  resolveCloseRequest,
  resolveTaskAttention,
  resumeTask,
  retryTask,
  runTrayMenuAction,
  updateTaskTransferOptions,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { taskCursorInput, useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { UNDO_TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

/**
 * `CompletionAction` includes `none`, which has no dialog copy and never opens
 * this dialog. Keying these maps on the excluded union replaces the previous
 * capitalize-style template construction, which the compiler could not check.
 */
type CompletionDialogAction = Exclude<CompletionAction, "none">;

const COMPLETION_TITLE_KEYS = {
  exit_app: "completionDialog.exit_appTitle",
  shutdown: "completionDialog.shutdownTitle",
  sleep: "completionDialog.sleepTitle",
  hibernate: "completionDialog.hibernateTitle",
  lock_screen: "completionDialog.lock_screenTitle",
  run_command: "completionDialog.run_commandTitle",
} as const satisfies Record<CompletionDialogAction, TranslationKey>;

const COMPLETION_DESCRIPTION_KEYS = {
  exit_app: "completionDialog.exit_appDescription",
  shutdown: "completionDialog.shutdownDescription",
  sleep: "completionDialog.sleepDescription",
  hibernate: "completionDialog.hibernateDescription",
  lock_screen: "completionDialog.lock_screenDescription",
  run_command: "completionDialog.run_commandDescription",
} as const satisfies Record<CompletionDialogAction, TranslationKey>;

const COMPLETION_CONFIRM_KEYS = {
  exit_app: "completionDialog.confirmExit",
  shutdown: "completionDialog.confirmShutdown",
  sleep: "completionDialog.confirmSleep",
  hibernate: "completionDialog.confirmHibernate",
  lock_screen: "completionDialog.confirmLock",
  run_command: "completionDialog.confirmRun",
} as const satisfies Record<CompletionDialogAction, TranslationKey>;

interface NewDownloadInitialState {
  sourceId: string;
  url?: string;
  batchInput?: string;
}

const TaskDetails = lazy(() =>
  import("@/components/shell/TaskDetails").then((module) => ({
    default: module.TaskDetails,
  })),
);
const Palette = lazy(() =>
  import("@/components/shell/Palette").then((module) => ({
    default: module.Palette,
  })),
);
const NewDownloadDialog = lazy(() =>
  import("@/components/shell/NewDownloadDialog").then((module) => ({
    default: module.NewDownloadDialog,
  })),
);
const DeleteTaskDialog = lazy(() =>
  import("@/components/shell/DeleteTaskDialog").then((module) => ({
    default: module.DeleteTaskDialog,
  })),
);
const BulkDeleteDialog = lazy(() =>
  import("@/components/shell/BulkDeleteDialog").then((module) => ({
    default: module.BulkDeleteDialog,
  })),
);
const ResolveAttentionDialog = lazy(() =>
  import("@/components/shell/ResolveAttentionDialog").then((module) => ({
    default: module.ResolveAttentionDialog,
  })),
);
const ShortcutPanel = lazy(() =>
  import("@/components/shell/ShortcutPanel").then((module) => ({
    default: module.ShortcutPanel,
  })),
);
const OnboardingDialog = lazy(() =>
  import("@/components/shell/OnboardingDialog").then((module) => ({
    default: module.OnboardingDialog,
  })),
);

function matchesShortcut(event: KeyboardEvent, shortcut: string, platform: Platform): boolean {
  const parts = shortcut.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  if (parts.includes("mod")) {
    const modOk = platform === "macos" ? event.metaKey : event.ctrlKey;
    if (!modOk) return false;
  }
  if (parts.includes("shift") && !event.shiftKey) return false;
  if (!parts.includes("shift") && event.shiftKey) return false;
  return event.key.toLowerCase() === key;
}

export function AppShell() {
  const { t } = useTranslation();
  const [platform, setPlatform] = useState<Platform>("unknown");
  const [paletteOpen, setPaletteOpen] = useState(false);
  // UX-3: Ref for mod+f / "/" to focus the search input in CommandBar.
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [shortcutPanelOpen, setShortcutPanelOpen] = useState(false);
  const [newDownloadOpen, setNewDownloadOpen] = useState(false);
  const newDownloadReturnFocusRef = useRef<HTMLElement | null>(null);
  const [newDownloadInitialState, setNewDownloadInitialState] = useState<NewDownloadInitialState | null>(null);
  const [newDownloadDraftDirty, setNewDownloadDraftDirty] = useState(false);
  const [newDownloadCreating, setNewDownloadCreating] = useState(false);
  const [detailAnnouncement, setDetailAnnouncement] = useState("");
  // Hard-confirm dialogs are only used for "delete files too" (irreversible).
  // Metadata-only removal goes through an undoable soft-delete flow.
  const [deleteFilesTarget, setDeleteFilesTarget] = useState<Task | null>(null);
  const [bulkDeleteFilesTargets, setBulkDeleteFilesTargets] = useState<Task[]>([]);
  const [attentionRequest, setAttentionRequest] = useState<AttentionDialogRequest | null>(null);
  const [completionActionRequest, setCompletionActionRequest] = useState<CompletionActionRequestedPayload | null>(null);
  const [closeRequest, setCloseRequest] = useState<CloseRequestPayload | null>(null);
  const [onboardingOpen, setOnboardingOpen] = useState(false);

  const selectedId = useTaskUIStore((s) => s.selectedId);
  const taskIds = useTaskDataStore((s) => s.taskIds);
  const addPendingDelete = useTaskUIStore((s) => s.addPendingDelete);
  const addPendingDeletes = useTaskUIStore((s) => s.addPendingDeletes);
  const removePendingDelete = useTaskUIStore((s) => s.removePendingDelete);
  // Read selected task imperatively via getState() in callbacks/handlers rather
  // than subscribing here — subscribing would put AppShell on the per-tick
  // re-render path (task object changes every 250ms progress tick).
  const nav = useTaskUIStore((s) => s.nav);
  const detailOpen = useTaskUIStore((s) => s.detailOpen);
  const setTaskCursorPage = useTaskDataStore((s) => s.setTaskCursorPage);
  const upsertTask = useTaskDataStore((s) => s.upsertTask);
  const setError = useTaskDataStore((s) => s.setError);
  const selectTask = useTaskUIStore((s) => s.selectTask);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setNav = useTaskUIStore((s) => s.setNav);
  const setDetailOpen = useTaskUIStore((s) => s.setDetailOpen);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setSettingsLoading = useSettingsStore((s) => s.setLoading);
  const setSettingsError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);
  const updateToast = useToastStore((s) => s.updateToast);
  const { selectAllMatching, selectingAll } = useSelectAllMatching();

  // The attention view is an ordinary list now (one of the "Needs you" cause
  // filters), so it keeps search, sort, filters, and the details panel.
  const taskSurfaceActive =
    nav !== "settings" &&
    nav !== "about" &&
    nav !== "queue" &&
    nav !== "storage" &&
    nav !== "recovery" &&
    nav !== "backup";

  const refreshTasks = useCallback(
    async (selectId?: string) => {
      // ARC-07: share epoch with TaskList so a late refresh cannot overwrite a newer query.
      const epoch = bumpListQueryEpoch();
      try {
        const page = await listTasksCursor(taskCursorInput(null));
        if (!isCurrentListQueryEpoch(epoch)) return;
        const data = page.items;
        setTaskCursorPage(data, page.minimumTotal, page.nextCursor, page.filterOptions);
        if (selectId) {
          selectTask(selectId);
        } else {
          const currentSelectedId = useTaskUIStore.getState().selectedId;
          if (data.length > 0 && (!currentSelectedId || !data.some((task) => task.id === currentSelectedId))) {
            selectTask(data[0].id);
          } else if (data.length === 0) {
            selectTask(null);
          }
        }
      } catch (err) {
        // UX-25: refreshTasks is invoked fire-and-forget from several surfaces
        // (context-menu Refresh, reorder revert); a rejection here would
        // otherwise surface as a silent unhandled rejection.
        log.error("refreshTasks failed", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: localizedErrorMessage(err, t),
        });
      }
    },
    [addToast, selectTask, setTaskCursorPage, t],
  );

  const runTaskAction = useCallback(
    async (
      action: () => Promise<Task | void>,
      selectId?: string,
      options?: { suppressErrorToast?: boolean },
    ): Promise<boolean> => {
      try {
        const result = await action();
        setError(null);
        if (result) {
          upsertTask(result);
          if (selectId) selectTask(selectId);
        } else {
          await refreshTasks(selectId);
        }
        return true;
      } catch (err) {
        const message = localizedErrorMessage(err, t);
        log.error("task action failed", err);
        setError(message);
        if (!options?.suppressErrorToast) {
          addToast({
            tone: "error",
            title: t("toast.actionFailed"),
            description: message,
          });
        }
        return false;
      }
    },
    [addToast, refreshTasks, selectTask, setError, t, upsertTask],
  );

  const toggleTransfer = useCallback(
    (task: Task) => {
      const action = allowedTransferActions(task)[0];
      if (action === "pause") {
        if (pauseWouldDiscardProgress(task)) {
          setAttentionRequest({ task, action: "pause" });
          return;
        }
        void runTaskAction(() => pauseTask(task.id), task.id);
      } else if (action === "resume") {
        void runTaskAction(() => resumeTask(task.id), task.id);
      }
    },
    [runTaskAction],
  );

  const retry = useCallback(
    (task: Task) => {
      if (allowedTransferActions(task).includes("retry")) {
        void runTaskAction(() => retryTask(task.id), task.id);
      }
    },
    [runTaskAction],
  );

  const redownload = useCallback(
    (task: Task) => {
      if (task.status === "completed") {
        void runTaskAction(() => redownloadTask(task.id), undefined);
      }
    },
    [runTaskAction],
  );

  const recheck = useCallback(
    (task: Task) => {
      if (task.status !== "completed") return;
      void runTaskAction(() => recheckTask(task.id), task.id);
    },
    [runTaskAction],
  );

  const handleReorder = useCallback(
    async (task: Task, action: ReorderAction) => {
      // Only same-priority Queued tasks participate in reordering. We read
      // from store.getState() to always see the latest filtered/sorted list
      // without re-creating the callback on every store change.
      const state = useTaskDataStore.getState();
      const samePriorityQueued = state.taskIds
        .map((id) => state.taskById[id])
        .filter((t): t is Task => Boolean(t) && t.status === "queued" && t.priority === task.priority);
      if (samePriorityQueued.length === 0) return;
      const currentIndex = samePriorityQueued.findIndex((t) => t.id === task.id);
      if (currentIndex === -1) return;
      const next = [...samePriorityQueued];
      const [moved] = next.splice(currentIndex, 1);
      switch (action) {
        case "move_to_top":
          next.unshift(moved);
          break;
        case "move_up":
          next.splice(Math.max(0, currentIndex - 1), 0, moved);
          break;
        case "move_down":
          next.splice(Math.min(next.length, currentIndex + 1), 0, moved);
          break;
        case "move_to_bottom":
          next.push(moved);
          break;
      }
      const orderedIds = next.map((t) => t.id);
      // UX-5: Optimistic update — reorder the store immediately so the user
      // sees instant feedback. On failure, roll back to the pre-reorder order.
      const rollbackReorder = useTaskDataStore.getState().reorderTasksLocally(orderedIds);
      try {
        await reorderQueuedTasks(orderedIds);
      } catch (err) {
        log.error("reorder_queued_tasks failed, reverting", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: localizedErrorMessage(err, t),
        });
        // UX-20: restore the pre-reorder order right away, then reload the
        // authoritative server order. The previous `setLoading(true)` had no
        // observer, so both the optimistic order and the loading flag stayed
        // stuck until some unrelated event triggered a fetch.
        rollbackReorder?.();
        void refreshTasks();
      }
    },
    [addToast, refreshTasks, t],
  );

  const finishRecording = useCallback(
    (task: Task) => {
      void runTaskAction(() => finishLiveRecording(task.id), task.id);
    },
    [runTaskAction],
  );

  const openFile = useCallback(
    (task: Task) => {
      void runTaskAction(() => openTaskFile(task.id), task.id);
    },
    [runTaskAction],
  );

  const openFolder = useCallback(
    (task: Task) => {
      void runTaskAction(() => openTaskFolder(task.id), task.id);
    },
    [runTaskAction],
  );

  const copyTaskUrl = useCallback(
    async (task: Task) => {
      try {
        await writeClipboardText(task.url);
        addToast({ tone: "success", title: t("contextmenu.task.urlCopied") });
      } catch (err) {
        log.warn("copy url failed", err);
        addToast({ tone: "error", title: t("contextmenu.task.copyFailed") });
      }
    },
    [addToast, t],
  );

  const copyTaskLocalPath = useCallback(
    async (task: Task) => {
      const path = task.finalPath ?? `${task.saveDir}/${task.fileName}`;
      try {
        await writeClipboardText(path);
        addToast({ tone: "success", title: t("contextmenu.task.pathCopied") });
      } catch (err) {
        log.warn("copy path failed", err);
        addToast({ tone: "error", title: t("contextmenu.task.copyFailed") });
      }
    },
    [addToast, t],
  );

  const showTaskDetails = useCallback(
    (task: Task) => {
      const currentNav = useTaskUIStore.getState().nav;
      if (currentNav === "queue") setNav("all");
      selectTask(task.id);
      setDetailOpen(true);
      setDetailAnnouncement(t("taskDetails.openedAnnouncement", { name: task.fileName }));
    },
    [selectTask, setDetailOpen, setNav, t],
  );

  const updateQueueOptions = useCallback(
    (task: Task, patch: { priority?: TaskPriority; obeySchedule?: boolean }) =>
      runTaskAction(
        () =>
          updateTaskTransferOptions({
            id: task.id,
            taskSpeedLimitBps: task.taskSpeedLimitBps == null ? null : String(task.taskSpeedLimitBps),
            priority: patch.priority ?? task.priority,
            queuePosition: null,
            categoryKey: task.categoryKey,
            obeySchedule: patch.obeySchedule ?? task.obeySchedule,
          }),
        task.id,
      ),
    [runTaskAction],
  );

  const refreshTaskList = useCallback(() => {
    void refreshTasks();
  }, [refreshTasks]);

  // Single-IPC bulk transfer action (pause/resume/retry) via bulk_task_action.
  // Replaces N serial IPC calls with one aggregated call + one refresh.
  // Uses toast key deduplication so consecutive bulk operations update the
  // existing toast instead of stacking new ones.
  const runBulkTransferAction = useCallback(
    async (ids: string[], action: "pause" | "resume" | "retry", label: string) => {
      if (ids.length === 0) return;
      const total = ids.length;
      const toastKey = `bulk-${action}`;
      const toastId = addToast({
        tone: "info",
        title: t("toast.bulkProgress", { action: label, done: 0, total }),
        key: toastKey,
      });
      try {
        const succeeded = await bulkTaskAction(ids, action);
        const failed = total - succeeded;
        await refreshTasks();
        if (failed === 0) {
          updateToast(toastId, {
            tone: "success",
            title: t("toast.bulkComplete", { action: label, done: succeeded, total }),
          });
        } else if (failed === total) {
          updateToast(toastId, {
            tone: "error",
            title: t("toast.bulkFailed", { action: label }),
            description: t("toast.bulkFailureDetail", { failed, total }),
          });
        } else {
          updateToast(toastId, {
            tone: "error",
            title: t("toast.bulkPartialFailure", { action: label, failed, total }),
          });
        }
      } catch (err) {
        const message = localizedErrorMessage(err, t);
        updateToast(toastId, {
          tone: "error",
          title: t("toast.bulkFailed", { action: label }),
          description: message,
        });
      }
    },
    [addToast, updateToast, refreshTasks, t],
  );

  const bulkPause = useCallback(
    (selectedTasks: Task[]) => {
      const pauseable = selectedTasks.filter((task) => allowedTransferActions(task).includes("pause"));
      const skipped = pauseable.filter(pauseWouldDiscardProgress).length;
      const ids = pauseable.filter((task) => !pauseWouldDiscardProgress(task)).map((task) => task.id);
      if (skipped > 0) {
        addToast({ tone: "info", title: t("toast.bulkSkippedResumeUnavailable", { skipped }) });
      }
      void runBulkTransferAction(ids, "pause", t("taskList.bulkPause"));
    },
    [addToast, runBulkTransferAction, t],
  );

  const bulkResume = useCallback(
    (selectedTasks: Task[]) => {
      const ids = selectedTasks
        .filter((task) => allowedTransferActions(task).includes("resume"))
        .map((task) => task.id);
      void runBulkTransferAction(ids, "resume", t("taskList.bulkResume"));
    },
    [runBulkTransferAction, t],
  );

  const bulkRetry = useCallback(
    (selectedTasks: Task[]) => {
      const ids = selectedTasks.filter((task) => allowedTransferActions(task).includes("retry")).map((task) => task.id);
      void runBulkTransferAction(ids, "retry", t("taskList.bulkRetry"));
    },
    [runBulkTransferAction, t],
  );

  // UX-05: pause/resume every matching task in the DB, not the loaded page.
  const pauseAll = useCallback(async () => {
    const label = t("taskList.pauseAll");
    const toastKey = "bulk-pause-all";
    const toastId = addToast({
      tone: "info",
      title: t("toast.bulkProgress", { action: label, done: 0, total: "…" }),
      key: toastKey,
    });
    try {
      const result = await bulkTaskActionGlobal("pause");
      const total = result.succeeded + result.skipped + result.failed;
      await refreshTasks();
      if (result.failed === 0) {
        updateToast(toastId, {
          tone: "success",
          title: t("toast.bulkComplete", { action: label, done: result.succeeded, total }),
          description:
            result.skipped > 0 ? t("toast.bulkSkippedResumeUnavailable", { skipped: result.skipped }) : undefined,
        });
      } else {
        updateToast(toastId, {
          tone: "error",
          title: t("toast.bulkPartialFailure", {
            action: label,
            failed: result.failed,
            total,
          }),
          description:
            result.skipped > 0
              ? t("toast.bulkSkippedResumeUnavailable", { skipped: result.skipped })
              : t("toast.bulkFailureDetail", { failed: result.failed, total }),
        });
      }
    } catch (err) {
      updateToast(toastId, {
        tone: "error",
        title: t("toast.bulkFailed", { action: label }),
        description: localizedErrorMessage(err, t),
      });
    }
  }, [addToast, updateToast, refreshTasks, t]);

  const resumeAll = useCallback(async () => {
    const label = t("taskList.resumeAll");
    const toastKey = "bulk-resume-all";
    const toastId = addToast({
      tone: "info",
      title: t("toast.bulkProgress", { action: label, done: 0, total: "…" }),
      key: toastKey,
    });
    try {
      const result = await bulkTaskActionGlobal("resume");
      const total = result.succeeded + result.skipped + result.failed;
      await refreshTasks();
      if (result.failed === 0) {
        updateToast(toastId, {
          tone: "success",
          title: t("toast.bulkComplete", { action: label, done: result.succeeded, total }),
          description: result.skipped > 0 ? t("toast.bulkSkippedDetail", { skipped: result.skipped }) : undefined,
        });
      } else {
        updateToast(toastId, {
          tone: "error",
          title: t("toast.bulkPartialFailure", {
            action: label,
            failed: result.failed,
            total,
          }),
          description: t("toast.bulkSkippedDetail", { skipped: result.skipped }),
        });
      }
    } catch (err) {
      updateToast(toastId, {
        tone: "error",
        title: t("toast.bulkFailed", { action: label }),
        description: localizedErrorMessage(err, t),
      });
    }
  }, [addToast, updateToast, refreshTasks, t]);

  const bulkOpenFolder = useCallback(
    (selectedTasks: Task[]) => {
      const first = selectedTasks[0];
      if (first) openFolder(first);
    },
    [openFolder],
  );

  const bulkExport = useCallback(
    async (selectedTasks: Task[], format: "json" | "csv") => {
      if (selectedTasks.length === 0) return;
      try {
        const { exportTasks } = await import("@/lib/export");
        const success = await exportTasks(selectedTasks, format);
        if (success) {
          addToast({ tone: "success", title: t("taskList.exportSuccess", { count: selectedTasks.length }) });
        }
      } catch (err) {
        addToast({
          tone: "error",
          title: t("taskList.exportFailed"),
          description: localizedErrorMessage(err, t),
        });
      }
    },
    [addToast, t],
  );

  // ── Soft-delete (metadata only) with undo ──
  // Hides the task immediately via pendingDeleteIds and commits the hard delete
  // when the undo toast settles without Undo (timeout, dismiss, or clear). The
  // toast hover/focus pause is the only clock — no separate commit timer.
  const softDelete = useCallback(
    (task: Task) => {
      const id = task.id;
      if (useTaskUIStore.getState().pendingDeleteIds.includes(id)) return;
      addPendingDelete(id);
      let settled = false;
      const commit = () => {
        if (settled) return;
        settled = true;
        void (async () => {
          try {
            await deleteTask(id, false);
            removePendingDelete(id);
            // PERF-18: drop the entity from the cache so a confirmed delete
            // does not linger in taskById until the next page load.
            useTaskDataStore.getState().evictTasks([id]);
          } catch (err) {
            log.error("soft-delete commit failed", err);
            removePendingDelete(id);
            addToast({
              tone: "error",
              title: t("toast.actionFailed"),
              description: localizedErrorMessage(err, t),
            });
          }
        })();
      };
      const undo = () => {
        if (settled) return;
        settled = true;
        removePendingDelete(id);
      };
      addToast({
        tone: "info",
        title: t("toast.taskDeleted", { name: task.fileName }),
        description: t("toast.undoHint"),
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        key: `soft-delete-${id}`,
        onAutoCommit: commit,
        action: {
          label: t("toast.undo"),
          onClick: undo,
        },
      });
    },
    [addPendingDelete, addToast, removePendingDelete, t],
  );

  const softDeleteBulk = useCallback(
    (selectedTasks: Task[]) => {
      if (selectedTasks.length === 0) return;
      const alreadyPending = new Set(useTaskUIStore.getState().pendingDeleteIds);
      const fresh = selectedTasks.filter((task) => !alreadyPending.has(task.id));
      if (fresh.length === 0) return;
      const freshIds = fresh.map((task) => task.id);
      addPendingDeletes(freshIds);
      const label = t("taskList.bulkDelete", { count: fresh.length });
      let settled = false;
      const commit = () => {
        if (settled) return;
        settled = true;
        void (async () => {
          try {
            await bulkDeleteTasks(freshIds, false);
            for (const id of freshIds) removePendingDelete(id);
            useTaskDataStore.getState().evictTasks(freshIds);
            addToast({
              tone: "success",
              title: t("toast.bulkComplete", { action: label, done: freshIds.length, total: freshIds.length }),
              key: "bulk-delete",
            });
          } catch (err) {
            log.error("soft-delete bulk commit failed", err);
            for (const id of freshIds) removePendingDelete(id);
            addToast({
              tone: "error",
              title: t("toast.bulkFailed", { action: label }),
              description: localizedErrorMessage(err, t),
              key: "bulk-delete",
            });
          }
        })();
      };
      const undo = () => {
        if (settled) return;
        settled = true;
        for (const id of freshIds) removePendingDelete(id);
      };
      addToast({
        tone: "info",
        title: t("toast.tasksDeleted", { count: fresh.length }),
        description: t("toast.undoHint"),
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        key: "bulk-soft-delete",
        onAutoCommit: commit,
        action: {
          label: t("toast.undo"),
          onClick: undo,
        },
      });
    },
    [addPendingDeletes, addToast, removePendingDelete, t],
  );

  const bulkDelete = useCallback(
    (selectedTasks: Task[]) => {
      // Default bulk delete is undoable (metadata only).
      softDeleteBulk(selectedTasks);
      clearSelectedIds();
    },
    [clearSelectedIds, softDeleteBulk],
  );

  const bulkDeleteFiles = useCallback((selectedTasks: Task[]) => {
    if (selectedTasks.length === 0) return;
    setBulkDeleteFilesTargets(selectedTasks);
  }, []);

  const confirmBulkDeleteFiles = useCallback(() => {
    const targets = bulkDeleteFilesTargets;
    setBulkDeleteFilesTargets([]);
    if (targets.length === 0) return;
    void (async () => {
      const total = targets.length;
      const label = t("taskList.bulkDelete", { count: total });
      const ids = targets.map((task) => task.id);
      try {
        const done = await bulkDeleteTasks(ids, true);
        useTaskDataStore.getState().evictTasks(ids);
        addToast({
          tone: "success",
          title: t("toast.bulkComplete", { action: label, done, total }),
          key: "bulk-delete-files",
        });
      } catch (error) {
        addToast({
          tone: "error",
          title: t("toast.bulkFailed", { action: label }),
          description: localizedErrorMessage(error, t),
          key: "bulk-delete-files",
        });
      }
      clearSelectedIds();
    })();
  }, [addToast, bulkDeleteFilesTargets, clearSelectedIds, t]);

  const openNewDownload = useCallback((initialState?: NewDownloadInitialState) => {
    const active = document.activeElement;
    newDownloadReturnFocusRef.current = active instanceof HTMLElement && active !== document.body ? active : null;
    if (initialState) {
      setNewDownloadInitialState(initialState);
    } else {
      setNewDownloadInitialState(null);
    }
    setNewDownloadOpen(true);
  }, []);

  const handleNewDownloadOpenChange = useCallback((open: boolean) => {
    setNewDownloadOpen(open);
  }, []);

  const handleNewDownloadCloseAutoFocus = useCallback((event: Event) => {
    const target = newDownloadReturnFocusRef.current;
    newDownloadReturnFocusRef.current = null;
    if (target?.isConnected && !isModalFocusActive()) {
      // Restore only after Radix has released the dialog's focus trap.
      event.preventDefault();
      target.focus({ preventScroll: true });
    }
  }, []);

  // Stable shell handlers so memoized children (TaskList / TaskRow) are not
  // invalidated by fresh arrow functions on every AppShell render.
  const openPalette = useCallback(() => setPaletteOpen(true), []);
  const openShortcutPanel = useCallback(() => setShortcutPanelOpen(true), []);
  const openOnboarding = useCallback(() => setOnboardingOpen(true), []);
  const openAbout = useCallback(() => setNav("about"), [setNav]);
  const requestDeleteFiles = useCallback((task: Task) => setDeleteFilesTarget(task), []);

  // Stable identity so the lazily loaded details panel does not re-render on
  // every AppShell render just because a fresh object was passed.
  const detailActions = useMemo(
    () => ({
      onToggleTransfer: toggleTransfer,
      onRetry: retry,
      onRedownload: redownload,
      onRecheck: recheck,
      onOpenFile: openFile,
      onOpenFolder: openFolder,
      onDelete: softDelete,
    }),
    [toggleTransfer, retry, redownload, recheck, openFile, openFolder, softDelete],
  );

  const applyClipboardDownload = useCallback(
    (sourceId: string, urls: string[]) => {
      if (urls.length === 0) return;
      openNewDownload({
        sourceId: `clipboard-${sourceId}`,
        url: urls.length === 1 ? urls[0] : undefined,
        batchInput: urls.length > 1 ? urls.join("\n") : undefined,
      });
    },
    [openNewDownload],
  );

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onBrowserHandoffAuthorizationRequired(({ requestId, url }) => {
      if (cancelled) return;
      openNewDownload({ sourceId: `browser-${requestId}`, url });
    }).then((cleanup) => {
      if (cancelled) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [openNewDownload]);

  const pasteAndCreate = useCallback(() => {
    void (async () => {
      const text = await navigator.clipboard?.readText().catch(() => null);
      if (text?.trim()) {
        const urls = text
          .split(/\s+/)
          .map((s) => s.trim())
          .filter(Boolean);
        applyClipboardDownload(crypto.randomUUID(), urls);
      } else {
        openNewDownload();
      }
    })();
  }, [applyClipboardDownload, openNewDownload]);

  const submitAttentionResolution = useCallback(
    (
      task: Task,
      action: RecoveryAction,
      overrides?: Partial<Pick<ResolveTaskAttentionInput, "fileName" | "saveDir">>,
    ) => {
      const input: ResolveTaskAttentionInput = {
        id: task.id,
        action,
        fileName: overrides?.fileName ?? null,
        saveDir: overrides?.saveDir ?? null,
      };

      void runTaskAction(() => resolveTaskAttention(input), task.id);
    },
    [runTaskAction],
  );

  const runCompletionAction = useCallback(
    async (request: CompletionActionRequestedPayload) => {
      setCompletionActionRequest(null);
      try {
        if (request.action === "exit_app") {
          await runTrayMenuAction("quit");
        } else if (request.action === "shutdown") {
          await requestSystemShutdown();
        } else if (request.action === "sleep") {
          await requestSystemSleep();
        } else if (request.action === "hibernate") {
          await requestSystemHibernate();
        } else if (request.action === "lock_screen") {
          await requestLockScreen();
        }
      } catch (err) {
        const message = localizedErrorMessage(err, t);
        log.error("completion action failed", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: message,
        });
      }
    },
    [addToast, t],
  );

  const resolveCloseDecision = useCallback(
    async (action: Exclude<CloseRequestAction, "cancel">, remember: boolean) => {
      try {
        await resolveCloseRequest(action, remember);
        setCloseRequest(null);
      } catch (err) {
        log.error("close request resolution failed", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: localizedErrorMessage(err, t),
        });
      }
    },
    [addToast, t],
  );

  const resolveAttention = useCallback(
    async (task: Task, action: RecoveryAction) => {
      if (action === "open_folder" || action === "free_disk_space") {
        openFolder(task);
        if (action === "free_disk_space") {
          // Query disk space and surface the available bytes + shortfall so
          // the user knows how much to free before retrying.
          let description = task.saveDir;
          try {
            const info = await queryDiskSpace(task.saveDir);
            const availableBytes = Number(info.available_bytes);
            const availableLabel = formatBytes(availableBytes);
            const totalSize = task.totalSize ? Number(task.totalSize) : 0;
            if (totalSize > 0) {
              const downloaded = task.downloadedBytes ? Number(task.downloadedBytes) : 0;
              const remainingNeeded = Math.max(0, totalSize - downloaded);
              const shortfall = Math.max(0, remainingNeeded - availableBytes);
              if (shortfall > 0) {
                description = t("recovery.diskSpaceShortfall", {
                  available: availableLabel,
                  needed: formatBytes(remainingNeeded),
                  shortfall: formatBytes(shortfall),
                });
              } else {
                description = t("recovery.diskSpaceAvailable", {
                  available: availableLabel,
                  needed: formatBytes(remainingNeeded),
                });
              }
            } else {
              description = t("recovery.diskSpaceUnknown", { available: availableLabel });
            }
          } catch (err) {
            log.warn("query_disk_space failed", err);
          }
          addToast({
            tone: "info",
            title: t("recovery.freeDiskSpaceToast"),
            description,
            action: {
              label: t("actions.retry"),
              onClick: () => submitAttentionResolution(task, "retry"),
            },
            durationMs: UNDO_TOAST_TIMEOUT_MS,
          });
        }
        return;
      }
      if (action === "check_url") {
        setNav("all");
        selectTask(task.id);
        setDetailOpen(true);
        addToast({
          tone: "info",
          title: torrentFileSelectionRequired(task) ? t("newDownload.chooseFile") : t("recovery.checkUrlToast"),
          description: torrentFileSelectionRequired(task)
            ? t("errors.btFileSelectionRequired")
            : sanitizeUrlForDisplay(task.url),
        });
        return;
      }

      if (action === "configure_ffmpeg") {
        writeSettingsRecoveryReturn({
          focus: "ffmpeg_path",
          taskId: task.id,
          action: "configure_ffmpeg",
        });
        setNav("settings");
        addToast({
          tone: "info",
          title: t("settings.ffmpegPath.label"),
          description: t("settings.ffmpegPath.notDetected"),
        });
        return;
      }

      if (action === "manage_sftp_host_keys") {
        writeSettingsRecoveryReturn({
          focus: "sftp_known_hosts",
          taskId: task.id,
          action: "manage_sftp_host_keys",
        });
        setNav("settings");
        addToast({
          tone: "info",
          title: t("settings.sftpKnownHosts"),
          description: t("recovery.manageSftpHostKeysToast"),
        });
        return;
      }

      if (action === "choose_another_name") {
        setAttentionRequest({ task, action });
        return;
      }

      if (action === "choose_another_folder") {
        // UX-25: the picker await runs outside any caller's try/catch, so an
        // IPC failure needs its own feedback path instead of dying silently.
        try {
          const saveDir = await openDirectoryPicker();
          if (!saveDir) return;
          submitAttentionResolution(task, action, { saveDir });
        } catch (err) {
          log.error("attention folder picker failed", err);
          addToast({
            tone: "error",
            title: t("toast.actionFailed"),
            description: localizedErrorMessage(err, t),
          });
        }
        return;
      }

      if (action === "restart") {
        setAttentionRequest({ task, action });
        return;
      }

      submitAttentionResolution(task, action);
    },
    [addToast, openFolder, selectTask, setDetailOpen, setNav, submitAttentionResolution, t],
  );

  // Mod+R and the palette run the same fix the row's banner offers, so a
  // restart-only failure is reachable from the keyboard. Restart keeps its
  // cost confirmation because it goes through resolveAttention like a click.
  const recoverTask = useCallback(
    (task: Task) => {
      const action = primaryRecoveryAction(task);
      if (!action) {
        addToast({ tone: "info", title: t("toast.nothingToRecover", { name: task.fileName }) });
        return;
      }
      if (action === "retry" && !hasInlineRecovery(task)) {
        retry(task);
        return;
      }
      void resolveAttention(task, action);
    },
    [addToast, resolveAttention, retry, t],
  );

  useEffect(() => {
    // UX-25: getPlatform already falls back internally; this catch is hygiene
    // so the one-off effect can never produce an unhandled rejection.
    getPlatform()
      .then(setPlatform)
      .catch((err) => log.warn("platform detection failed", err));
  }, []);

  useEffect(() => {
    try {
      if (localStorage.getItem("vibe-onboarding-completed") !== "1") {
        setOnboardingOpen(true);
      }
    } catch {
      // localStorage unavailable; skip onboarding
    }
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-platform", platform);
    document.documentElement.style.setProperty("--traffic-lights-inset", `${trafficLightsInsetPx(platform)}px`);
  }, [platform]);

  useTaskEvents();

  useEffect(() => {
    let cancelled = false;
    let unlistenSettings: (() => void) | undefined;

    async function refreshSettings() {
      try {
        const data = await getSettings();
        if (!cancelled) {
          setSettings(data);
          setSettingsError(null);
        }
      } catch (err) {
        if (!cancelled) {
          log.warn("settings load failed", err);
          setSettingsError(localizedErrorMessage(err, t));
        }
      } finally {
        if (!cancelled) setSettingsLoading(false);
      }
    }

    void (async () => {
      await refreshSettings();
      unlistenSettings = await onSettingsChanged(refreshSettings);
      if (cancelled) unlistenSettings();
    })();

    return () => {
      cancelled = true;
      unlistenSettings?.();
    };
  }, [setSettings, setSettingsError, setSettingsLoading, t]);

  useEffect(() => {
    let cancelled = false;
    let unlistenNewDownload: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;

    void (async () => {
      unlistenNewDownload = await onTrayNewDownloadRequested(() => {
        openNewDownload();
      });
      unlistenSettings = await onTraySettingsRequested(() => {
        useTaskUIStore.getState().setNav("settings");
        setDetailOpen(false);
      });

      if (cancelled) {
        unlistenNewDownload();
        unlistenSettings();
      }
    })();

    return () => {
      cancelled = true;
      unlistenNewDownload?.();
      unlistenSettings?.();
    };
  }, [openNewDownload, setDetailOpen]);

  // UX-23: the dialog-state read happens inside the handler on every event, so
  // the listener registers once and never misses the await-window between
  // unlisten and re-listen.
  //
  // UX-43: a clipboard detection only ever produces a toast (see the hook).
  // Its "Use link" action is the user's explicit request for this link, so
  // the dialog then probes exactly like a pasted URL; nothing reaches the
  // network before that click.
  useClipboardLinkPrompt((payload) => applyClipboardDownload(payload.id, payload.urls));

  useEffect(() => {
    let cancelled = false;
    let unlistenCompletionAction: (() => void) | undefined;

    void (async () => {
      unlistenCompletionAction = await onCompletionActionRequested((payload) => {
        if (payload.action === "none") return;
        setCompletionActionRequest(payload);
      });
      if (cancelled) unlistenCompletionAction?.();
    })();

    return () => {
      cancelled = true;
      unlistenCompletionAction?.();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      unlisten = await onCloseRequested((payload) => {
        if (!cancelled) setCloseRequest(payload);
      });
      if (cancelled) unlisten?.();
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const accent = settings?.accentColor ?? "blue";
    document.documentElement.dataset.accent = accent;
    try {
      localStorage.setItem("vibe-accent", accent);
    } catch {
      // localStorage unavailable; skip persistence
    }
  }, [settings?.accentColor]);

  const [dropActive, setDropActive] = useState(false);
  // Linux keeps native window chrome (no in-app titlebar to host the bar).
  const chromeLayout = useChromeLayout();
  const mergedChrome = chromeLayout === "merged" && platform !== "linux";

  const applyDroppedFile = useCallback((initialState: NewDownloadInitialState) => {
    setNewDownloadInitialState(initialState);
    setNewDownloadOpen(true);
  }, []);

  useFileDropMonitor({
    onDrop: async (paths) => {
      const supported = paths.filter((path) => isSupportedLocalFile(path.split(/[/\\]/).pop() ?? path));
      if (supported.length === 0) {
        addToast({
          tone: "error",
          title: t("toast.unsupportedDroppedFiles"),
          description: t("toast.unsupportedDroppedFilesDescription"),
        });
        return;
      }
      const firstPath = supported[0];
      const name = firstPath.split(/[/\\]/).pop() ?? firstPath;
      try {
        const resolved = await resolveLocalFile(firstPath, name);
        if (newDownloadDraftDirty) {
          addToast({
            tone: "info",
            title: t("toast.droppedFileReady"),
            description: name,
            action: {
              label: t("toast.useDroppedFile"),
              onClick: () =>
                applyDroppedFile({
                  sourceId: `drop-${Date.now()}`,
                  url: resolved.url,
                  batchInput: resolved.batchInput,
                }),
            },
          });
          return;
        }
        applyDroppedFile({
          sourceId: `drop-${Date.now()}`,
          url: resolved.url,
          batchInput: resolved.batchInput,
        });
      } catch (err) {
        log.error("dropped file resolve failed", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: localizedErrorMessage(err, t),
        });
      }
    },
    onDragStateChange: (state) => setDropActive(state.active),
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // IME composition guard: when a CJK input method is composing (e.g. typing
      // pinyin), keystrokes should go to the IME, not trigger app shortcuts.
      if (event.isComposing) return;
      const target = event.target as HTMLElement;
      const isCheckbox = target instanceof HTMLInputElement && target.type === "checkbox";
      const isInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable;
      const inOverlay = isOverlayKey(event);

      // Read selected task imperatively — not a subscription, so AppShell
      // stays off the per-tick re-render path.
      const selected = selectedId ? (useTaskDataStore.getState().taskById[selectedId] ?? null) : null;

      // ── Always-active shortcuts ──

      if (matchesShortcut(event, "mod+k", platform)) {
        event.preventDefault();
        setPaletteOpen(true);
        return;
      }

      // UX-3: mod+f focuses the search input for keyboard-first users.
      if (matchesShortcut(event, "mod+f", platform)) {
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }

      if (matchesShortcut(event, "mod+/", platform)) {
        event.preventDefault();
        setShortcutPanelOpen((prev) => !prev);
        return;
      }

      if (matchesShortcut(event, "mod+n", platform)) {
        event.preventDefault();
        openNewDownload();
        return;
      }

      if (matchesShortcut(event, "mod+,", platform)) {
        event.preventDefault();
        setNav("settings");
        setDetailOpen(false);
        return;
      }

      if (matchesShortcut(event, "mod+shift+p", platform)) {
        event.preventDefault();
        void pauseAll();
        return;
      }
      if (matchesShortcut(event, "mod+shift+r", platform)) {
        event.preventDefault();
        void resumeAll();
        return;
      }

      if (isGlobalPasteShortcut(event, platform)) {
        event.preventDefault();
        pasteAndCreate();
        return;
      }

      // Details can be opened from a row, palette, or the command bar. Keep
      // Escape meaningful from the list surface as well as inside the drawer.
      if (!isInput && !inOverlay && event.key === "Escape" && detailOpen) {
        event.preventDefault();
        setDetailOpen(false);
        return;
      }

      // ── Non-input shortcuts ──

      // Checkboxes are row selection controls, not text-entry surfaces. Keep
      // bulk and delete shortcuts available while one has focus.
      if ((!isInput || isCheckbox) && !inOverlay) {
        if (event.key === "?") {
          event.preventDefault();
          setShortcutPanelOpen((prev) => !prev);
          return;
        }

        // Navigation: Mod+1–7 follows the sidebar order in nav-shortcuts.ts.
        const navTarget = navFilterForDigit(event.key);
        if (navTarget && matchesShortcut(event, `mod+${event.key}`, platform)) {
          event.preventDefault();
          setNav(navTarget);
          return;
        }

        // Task selection: Mod+Up / Mod+Down
        if (matchesShortcut(event, "mod+arrowup", platform)) {
          event.preventDefault();
          const idx = selectedId ? taskIds.indexOf(selectedId) : -1;
          if (idx > 0) selectTask(taskIds[idx - 1]!);
          else if (taskIds.length > 0 && idx === -1) selectTask(taskIds[0]!);
          return;
        }

        if (matchesShortcut(event, "mod+arrowdown", platform)) {
          event.preventDefault();
          const idx = selectedId ? taskIds.indexOf(selectedId) : -1;
          if (idx >= 0 && idx < taskIds.length - 1) selectTask(taskIds[idx + 1]!);
          else if (taskIds.length > 0 && idx === -1) selectTask(taskIds[0]!);
          return;
        }

        // Toggle detail: Mod+D
        if (matchesShortcut(event, "mod+d", platform)) {
          event.preventDefault();
          setDetailOpen(!detailOpen);
          return;
        }

        // Open folder: Mod+O
        if (matchesShortcut(event, "mod+o", platform) && selected) {
          event.preventDefault();
          openFolder(selected);
          return;
        }

        // Open file: Mod+Enter
        if (matchesShortcut(event, "mod+enter", platform) && selected) {
          event.preventDefault();
          openFile(selected);
          return;
        }

        // Recover the selected task: Mod+R. Swallowed even when there is
        // nothing to recover — the WebView would otherwise reload the whole
        // app, and a silent no-op read as a broken shortcut.
        if (matchesShortcut(event, "mod+r", platform) && selected) {
          event.preventDefault();
          recoverTask(selected);
          return;
        }

        // P0: Toggle pause/resume on the selected task: Mod+P.
        // PRODUCT.md lists "pause, resume … mostly from the keyboard" as a
        // success criterion; before this binding the only keyboard path was
        // Mod+R (retry-only). toggleTransfer itself no-ops on completed and
        // needs_attention states, so the shortcut is safe to fire universally.
        if (matchesShortcut(event, "mod+p", platform) && selected) {
          if (allowedTransferActions(selected).some((action) => action === "pause" || action === "resume")) {
            event.preventDefault();
            toggleTransfer(selected);
          }
          return;
        }

        // Delete (metadata only, undoable): Del
        if (event.key === "Delete" && event.shiftKey === false && selected) {
          event.preventDefault();
          softDelete(selected);
          return;
        }

        // UX-5: Queue reorder via keyboard: Alt+Up/Down (only for queued tasks)
        if (
          event.altKey &&
          !event.metaKey &&
          !event.ctrlKey &&
          selected &&
          selected.status === "queued" &&
          (event.key === "ArrowUp" || event.key === "ArrowDown")
        ) {
          event.preventDefault();
          handleReorder(selected, event.key === "ArrowUp" ? "move_up" : "move_down");
          return;
        }

        // Delete task and files (irreversible, hard confirm): Shift+Del
        if (event.key === "Delete" && event.shiftKey === true && selected) {
          event.preventDefault();
          setDeleteFilesTarget(selected);
          return;
        }

        // Select every task matching the current query: Mod+A. The shared
        // loader owns cursor pagination and invalidates stale page responses.
        if (taskSurfaceActive && matchesShortcut(event, "mod+a", platform)) {
          event.preventDefault();
          void selectAllMatching();
          return;
        }

        // Clear selection: Mod+Shift+A
        if (matchesShortcut(event, "mod+shift+a", platform)) {
          event.preventDefault();
          clearSelectedIds();
          return;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    clearSelectedIds,
    detailOpen,
    handleReorder,
    openFile,
    openFolder,
    openNewDownload,
    pasteAndCreate,
    platform,
    recoverTask,
    selectTask,
    selectedId,
    setDetailOpen,
    setNav,
    selectAllMatching,
    softDelete,
    taskSurfaceActive,
    taskIds,
    toggleTransfer,
    pauseAll,
    resumeAll,
  ]);

  // UX-02: Suppress WebView native context menus on bubble phase so Radix
  // triggers can preventDefault first. Capture-phase listeners ran before
  // Trigger handlers and broke every custom right-click menu.
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      const target = event.target;
      if (target instanceof Element) {
        // Keep native cut/copy/paste menus for text editing surfaces.
        if (target.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]')) {
          return;
        }
      }
      event.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);

  const commandBar = (
    <CommandBar
      platform={platform}
      onOpenPalette={openPalette}
      onNewDownload={openNewDownload}
      inputRef={searchInputRef}
      taskSurfaceActive={taskSurfaceActive}
      suppressFirstRunTip={onboardingOpen}
      embedded={mergedChrome}
    />
  );

  return (
    <div className="flex h-full flex-col">
      <LiveRegion>{detailAnnouncement}</LiveRegion>
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:left-3 focus:top-2 focus:z-[100] focus:rounded-md focus:bg-surface-popover focus:px-3 focus:py-2 focus:text-sm focus:font-medium focus:text-text-primary focus:shadow-[var(--shadow-popover)] focus:ring-2 focus:ring-accent-primary focus:outline-none"
      >
        {t("app.skipToMain")}
      </a>
      <TitleBar
        platform={platform}
        onOpenPalette={openPalette}
        onNewDownload={openNewDownload}
        onOpenShortcuts={openShortcutPanel}
        center={mergedChrome ? commandBar : undefined}
      />
      {mergedChrome ? null : commandBar}
      {/* The shell changes composition at the same tiers as the navigation:
          a bottom bar on narrow windows, a compact rail on tablet widths, and
          the expandable rail on desktop. Keeping the list as the flex child
          lets it retain the available height in every tier. */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col md:flex-row">
        <Sidebar onNewDownload={openNewDownload} />
        <main
          id="main-content"
          tabIndex={-1}
          className="order-1 flex min-h-0 min-w-0 flex-1 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary md:order-2"
        >
          <TaskList
            onToggleTransfer={toggleTransfer}
            onRetry={retry}
            onRedownload={redownload}
            onRecheck={recheck}
            onFinishLiveRecording={finishRecording}
            onOpenFile={openFile}
            onOpenFolder={openFolder}
            onResolveAttention={resolveAttention}
            onDelete={softDelete}
            onDeleteFiles={requestDeleteFiles}
            onNewDownload={openNewDownload}
            onBulkPause={bulkPause}
            onBulkResume={bulkResume}
            onBulkRetry={bulkRetry}
            onBulkDelete={bulkDelete}
            onBulkDeleteFiles={bulkDeleteFiles}
            onBulkOpenFolder={bulkOpenFolder}
            onBulkExport={bulkExport}
            onOpenOnboarding={openOnboarding}
            onCopyUrl={copyTaskUrl}
            onCopyLocalPath={copyTaskLocalPath}
            onShowDetails={showTaskDetails}
            onPasteAndCreate={pasteAndCreate}
            onRefresh={refreshTaskList}
            onSelectAll={selectAllMatching}
            selectingAll={selectingAll}
            onReorder={handleReorder}
            onUpdateQueueOptions={updateQueueOptions}
            platform={platform}
          />
          <Suspense fallback={null}>
            <TaskDetails
              taskId={taskSurfaceActive ? selectedId : null}
              open={taskSurfaceActive && detailOpen && !!selectedId}
              onClose={() => {
                setDetailOpen(false);
                const focusId = selectedId;
                if (focusId) {
                  requestAnimationFrame(() => {
                    document.getElementById(`task-row-${focusId}`)?.focus();
                  });
                }
              }}
              onResolveAttention={resolveAttention}
              actions={detailActions}
            />
          </Suspense>
        </main>
      </div>
      <StatusBar
        className="flex"
        platform={platform}
        onOpenShortcuts={openShortcutPanel}
        onOpenAbout={openAbout}
        newDownloadState={newDownloadCreating ? "creating" : newDownloadDraftDirty ? "draft" : null}
        onOpenNewDownload={openNewDownload}
        onPauseAll={() => void pauseAll()}
        onResumeAll={() => void resumeAll()}
      />
      <ToastViewport />
      {paletteOpen ? (
        <Suspense fallback={null}>
          <Palette
            open={paletteOpen}
            onOpenChange={setPaletteOpen}
            platform={platform}
            selectedId={taskSurfaceActive ? selectedId : null}
            onNewDownload={openNewDownload}
            onStart={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task && allowedTransferActions(task).includes("resume")) {
                void runTaskAction(() => resumeTask(task.id), task.id);
              }
            }}
            onPause={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task) toggleTransfer(task);
            }}
            onDelete={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task) softDelete(task);
            }}
            onRecover={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task) recoverTask(task);
            }}
            onOpenFile={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task) openFile(task);
            }}
            onOpenFolder={() => {
              const task = selectedId ? useTaskDataStore.getState().taskById[selectedId] : null;
              if (task) openFolder(task);
            }}
            onBulkPause={bulkPause}
            onBulkResume={bulkResume}
            onBulkRetry={bulkRetry}
            onBulkDelete={bulkDelete}
            onBulkOpenFolder={bulkOpenFolder}
            onSelectAll={selectAllMatching}
            selectingAll={selectingAll}
            onPauseAll={() => void pauseAll()}
            onResumeAll={() => void resumeAll()}
            onSetNav={(nextNav) => {
              useTaskUIStore.getState().setNav(nextNav);
              if (nextNav === "settings") {
                setDetailOpen(false);
              }
            }}
          />
        </Suspense>
      ) : null}
      {/* UX-30: the dialog stays mounted for the session so its draft state
          survives close/reopen — the dialog itself decides (via its close
          guard) whether closing keeps the draft or just hides a busy create. */}
      <Suspense fallback={null}>
        <NewDownloadDialog
          open={newDownloadOpen}
          onOpenChange={handleNewDownloadOpenChange}
          onCloseAutoFocus={handleNewDownloadCloseAutoFocus}
          initialSourceId={newDownloadInitialState?.sourceId}
          initialUrl={newDownloadInitialState?.url}
          initialBatchInput={newDownloadInitialState?.batchInput}
          onDraftStateChange={setNewDownloadDraftDirty}
          onCreateStateChange={setNewDownloadCreating}
          onCreated={(task) => {
            useTaskDataStore.getState().upsertTask(task);
            selectTask(task.id);
          }}
        />
      </Suspense>
      {deleteFilesTarget ? (
        <Suspense fallback={null}>
          <DeleteTaskDialog
            task={deleteFilesTarget}
            open={!!deleteFilesTarget}
            onOpenChange={(open) => {
              if (!open) setDeleteFilesTarget(null);
            }}
            onDelete={() => {
              const target = deleteFilesTarget;
              setDeleteFilesTarget(null);
              if (target) {
                const id = target.id;
                void runTaskAction(async () => {
                  await deleteTask(id, true);
                  useTaskDataStore.getState().evictTasks([id]);
                });
              }
            }}
          />
        </Suspense>
      ) : null}
      {bulkDeleteFilesTargets.length > 0 ? (
        <Suspense fallback={null}>
          <BulkDeleteDialog
            tasks={bulkDeleteFilesTargets}
            open={bulkDeleteFilesTargets.length > 0}
            onOpenChange={(open) => {
              if (!open) setBulkDeleteFilesTargets([]);
            }}
            onDelete={confirmBulkDeleteFiles}
          />
        </Suspense>
      ) : null}
      {attentionRequest ? (
        <Suspense fallback={null}>
          <ResolveAttentionDialog
            request={attentionRequest}
            open={!!attentionRequest}
            onOpenChange={(open) => {
              if (!open) setAttentionRequest(null);
            }}
            onResolve={(fileName) => {
              const request = attentionRequest;
              setAttentionRequest(null);
              if (request?.action === "pause") {
                void runTaskAction(() => pauseTask(request.task.id), request.task.id);
              } else if (request) {
                submitAttentionResolution(request.task, request.action, {
                  fileName: fileName ?? null,
                });
              }
            }}
          />
        </Suspense>
      ) : null}
      {completionActionRequest ? (
        <CompletionActionDialog
          request={completionActionRequest}
          open={!!completionActionRequest}
          onCancel={() => setCompletionActionRequest(null)}
          onRun={runCompletionAction}
        />
      ) : null}
      <CloseDownloadDialog
        request={closeRequest}
        open={!!closeRequest}
        onCancel={() => {
          setCloseRequest(null);
          void resolveCloseRequest("cancel", false).catch((err) => {
            log.warn("close request cancel failed", err);
          });
        }}
        onAction={resolveCloseDecision}
      />
      {shortcutPanelOpen ? (
        <Suspense fallback={null}>
          <ShortcutPanel open={shortcutPanelOpen} onOpenChange={setShortcutPanelOpen} platform={platform} />
        </Suspense>
      ) : null}
      {onboardingOpen ? (
        <Suspense fallback={null}>
          <OnboardingDialog
            open={onboardingOpen}
            onOpenChange={setOnboardingOpen}
            onOpenSettings={() => {
              setNav("settings");
              setOnboardingOpen(false);
            }}
            onOpenNewDownload={() => setNewDownloadOpen(true)}
            platform={platform}
          />
        </Suspense>
      ) : null}
      {dropActive ? (
        <div
          className="drop-overlay pointer-events-none fixed inset-0 z-[60] flex items-center justify-center bg-surface-scrim motion-safe:animate-[fade-in_140ms_ease-out]"
          role="status"
          aria-label={t("toast.dropHere")}
        >
          <div className="flex flex-col items-center gap-3 rounded-xl border-2 border-dashed border-accent-primary bg-surface-base/80 px-10 py-8 text-center backdrop-blur-sm">
            <img src="/logo-64.png" alt="" width={40} height={40} className="select-none" draggable={false} />
            <span className="text-sm font-semibold text-text-primary">{t("toast.dropToStart")}</span>
            <span className="text-xs text-text-muted">{t("toast.dropHere")}</span>
          </div>
        </div>
      ) : null}
      <ShutdownOverlay />
    </div>
  );
}

function CompletionActionDialog({
  request,
  open,
  onCancel,
  onRun,
}: {
  request: CompletionActionRequestedPayload;
  open: boolean;
  onCancel: () => void;
  onRun: (request: CompletionActionRequestedPayload) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [remaining, setRemaining] = useState(request.countdownSeconds);
  const needsConfirm = request.action === "shutdown" || request.action === "sleep" || request.action === "hibernate";
  const hasCountdown = !needsConfirm;

  useEffect(() => {
    setRemaining(request.countdownSeconds);
  }, [request]);

  useEffect(() => {
    if (!open || !hasCountdown) return;
    if (remaining <= 0) {
      void onRun(request);
      return;
    }
    const timer = window.setTimeout(() => {
      setRemaining((value) => Math.max(0, value - 1));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [hasCountdown, onRun, open, remaining, request]);

  // Only a scheduled completion action opens this dialog, so `none` is excluded.
  const completionAction = request.action as CompletionDialogAction;
  const titleKey = COMPLETION_TITLE_KEYS[completionAction];
  const descriptionKey = COMPLETION_DESCRIPTION_KEYS[completionAction];
  const confirmKey = COMPLETION_CONFIRM_KEYS[completionAction];

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onCancel();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t(titleKey)}</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <DialogDescription>
            {hasCountdown ? t(descriptionKey, { seconds: remaining }) : t(descriptionKey)}
          </DialogDescription>
          {hasCountdown ? (
            <div
              role="progressbar"
              aria-label={t("completionDialog.countdownProgress")}
              aria-valuemin={0}
              aria-valuemax={Math.max(1, request.countdownSeconds)}
              aria-valuenow={remaining}
              aria-valuetext={t("completionDialog.secondsRemaining", { n: remaining })}
              className="h-1.5 overflow-hidden rounded-full bg-surface-raised"
            >
              <div
                className="h-full w-full origin-left rounded-full bg-accent-primary transition-transform duration-300 motion-reduce:transition-none"
                style={{
                  transform: `scaleX(${Math.max(0, Math.min(1, remaining / Math.max(1, request.countdownSeconds)))})`,
                }}
              />
            </div>
          ) : null}
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {t("completionDialog.cancel")}
          </Button>
          <Button variant={needsConfirm ? "danger" : "default"} onClick={() => void onRun(request)}>
            {t(confirmKey)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
