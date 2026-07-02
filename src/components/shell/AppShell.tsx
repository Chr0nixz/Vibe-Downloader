import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { CommandBar } from "@/components/shell/CommandBar";
import type { AttentionDialogRequest } from "@/components/shell/ResolveAttentionDialog";
import { ShutdownOverlay } from "@/components/shell/ShutdownOverlay";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
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
import { ToastViewport } from "@/components/ui/toast";
import type { CompletionActionRequestedPayload, RecoveryAction, ResolveTaskAttentionInput } from "@/generated/bindings";
import { useTaskEvents } from "@/hooks/use-task-events";
import { localizedErrorMessage } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { getPlatform, type Platform, trafficLightsInsetPx } from "@/lib/platform";
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";

const log = createLogger("app-shell");

import { isSupportedLocalFile, resolveLocalFile } from "@/lib/local-file";
import {
  bulkDeleteTasks,
  bulkTaskAction,
  deleteTask,
  type FileDropDragState,
  finishLiveRecording,
  getSettings,
  listTasksCursor,
  onClipboardLinkDetected,
  onCompletionActionRequested,
  onFileDrop,
  onSettingsChanged,
  onTrayNewDownloadRequested,
  onTraySettingsRequested,
  openDirectoryPicker,
  openTaskFile,
  openTaskFolder,
  pauseTask,
  queryDiskSpace,
  reorderQueuedTasks,
  requestLockScreen,
  requestSystemHibernate,
  requestSystemShutdown,
  requestSystemSleep,
  resolveTaskAttention,
  resumeTask,
  retryTask,
  runTrayMenuAction,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { taskCursorInput, useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { UNDO_TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

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
  const [newDownloadInitialState, setNewDownloadInitialState] = useState<NewDownloadInitialState | null>(null);
  const [newDownloadDraftDirty, setNewDownloadDraftDirty] = useState(false);
  // Hard-confirm dialogs are only used for "delete files too" (irreversible).
  // Metadata-only removal goes through an undoable soft-delete flow.
  const [deleteFilesTarget, setDeleteFilesTarget] = useState<Task | null>(null);
  const [bulkDeleteFilesTargets, setBulkDeleteFilesTargets] = useState<Task[]>([]);
  const [attentionRequest, setAttentionRequest] = useState<AttentionDialogRequest | null>(null);
  const [completionActionRequest, setCompletionActionRequest] = useState<CompletionActionRequestedPayload | null>(null);
  const [onboardingOpen, setOnboardingOpen] = useState(false);

  const selectedId = useTaskUIStore((s) => s.selectedId);
  const taskIds = useTaskDataStore((s) => s.taskIds);
  const totalTasks = useTaskDataStore((s) => s.total);
  const hasMoreTasks = useTaskDataStore((s) => s.hasMore);
  const pendingDeleteIds = useTaskUIStore((s) => s.pendingDeleteIds);
  const addPendingDelete = useTaskUIStore((s) => s.addPendingDelete);
  const addPendingDeletes = useTaskUIStore((s) => s.addPendingDeletes);
  const removePendingDelete = useTaskUIStore((s) => s.removePendingDelete);
  const selected = useTaskDataStore((s) => (selectedId ? (s.taskById[selectedId] ?? null) : null));
  const nav = useTaskUIStore((s) => s.nav);
  const detailOpen = useTaskUIStore((s) => s.detailOpen);
  const setTaskCursorPage = useTaskDataStore((s) => s.setTaskCursorPage);
  const upsertTask = useTaskDataStore((s) => s.upsertTask);
  const setError = useTaskDataStore((s) => s.setError);
  const selectTask = useTaskUIStore((s) => s.selectTask);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setSelectedIds = useTaskUIStore((s) => s.setSelectedIds);
  const setNav = useTaskUIStore((s) => s.setNav);
  const setDetailOpen = useTaskUIStore((s) => s.setDetailOpen);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setSettingsLoading = useSettingsStore((s) => s.setLoading);
  const setSettingsError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);
  const updateToast = useToastStore((s) => s.updateToast);

  const taskSurfaceActive = nav !== "settings" && nav !== "about";

  const refreshTasks = useCallback(
    async (selectId?: string) => {
      const page = await listTasksCursor(taskCursorInput(null));
      const data = page.items;
      setTaskCursorPage(data, page.totalEstimate, page.nextCursor, page.filterOptions);
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
    },
    [selectTask, setTaskCursorPage],
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
      if (task.status === "downloading" || task.status === "retrying" || task.status === "queued") {
        void runTaskAction(() => pauseTask(task.id), task.id);
      } else if (task.status !== "completed" && task.status !== "needs_attention") {
        void runTaskAction(() => resumeTask(task.id), task.id);
      }
    },
    [runTaskAction],
  );

  const retry = useCallback(
    (task: Task) => {
      void runTaskAction(() => retryTask(task.id), task.id);
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
      // sees instant feedback. On failure, reload from server to revert.
      useTaskDataStore.getState().reorderTasksLocally(orderedIds);
      try {
        await reorderQueuedTasks(orderedIds);
      } catch (err) {
        log.error("reorder_queued_tasks failed, reverting", err);
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: localizedErrorMessage(err, t),
        });
        // Revert: trigger a refresh to reload the correct order from the server.
        useTaskDataStore.getState().setLoading(true);
      }
    },
    [addToast, t],
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
        await navigator.clipboard?.writeText(task.url);
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
        await navigator.clipboard?.writeText(path);
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
      selectTask(task.id);
      setDetailOpen(true);
    },
    [selectTask, setDetailOpen],
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
      const ids = selectedTasks
        .filter((task) => task.status === "downloading" || task.status === "retrying" || task.status === "queued")
        .map((task) => task.id);
      void runBulkTransferAction(ids, "pause", t("taskList.bulkPause"));
    },
    [runBulkTransferAction, t],
  );

  const bulkResume = useCallback(
    (selectedTasks: Task[]) => {
      const ids = selectedTasks
        .filter((task) => task.status === "paused" || task.status === "failed" || task.status === "waiting_network")
        .map((task) => task.id);
      void runBulkTransferAction(ids, "resume", t("taskList.bulkResume"));
    },
    [runBulkTransferAction, t],
  );

  const bulkRetry = useCallback(
    (selectedTasks: Task[]) => {
      const ids = selectedTasks.filter((task) => task.status !== "completed").map((task) => task.id);
      void runBulkTransferAction(ids, "retry", t("taskList.bulkRetry"));
    },
    [runBulkTransferAction, t],
  );

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
      const { exportTasks } = await import("@/lib/export");
      const success = await exportTasks(selectedTasks, format);
      if (success) {
        addToast({ tone: "success", title: t("taskList.exportSuccess", { count: selectedTasks.length }) });
      }
    },
    [addToast, t],
  );

  // ── Soft-delete (metadata only) with undo ──
  // Hides the task immediately via pendingDeleteIds, shows an undo toast, and
  // commits the hard delete after the undo window elapses. Undo restores the
  // task before the commit fires.
  const softDelete = useCallback(
    (task: Task) => {
      const id = task.id;
      if (useTaskUIStore.getState().pendingDeleteIds.includes(id)) return;
      addPendingDelete(id);
      const commitTimer = window.setTimeout(() => {
        void (async () => {
          try {
            await deleteTask(id, false);
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
      }, UNDO_TOAST_TIMEOUT_MS);
      addToast({
        tone: "info",
        title: t("toast.taskDeleted", { name: task.fileName }),
        description: t("toast.undoHint"),
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        key: `soft-delete-${id}`,
        action: {
          label: t("toast.undo"),
          onClick: () => {
            window.clearTimeout(commitTimer);
            removePendingDelete(id);
          },
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
      const commitTimer = window.setTimeout(() => {
        void (async () => {
          try {
            await bulkDeleteTasks(freshIds, false);
            addToast({
              tone: "success",
              title: t("toast.bulkComplete", { action: label, done: freshIds.length, total: freshIds.length }),
              key: "bulk-delete",
            });
          } catch (err) {
            log.error("soft-delete bulk commit failed", err);
            // Delete failed — unhide the tasks so the user can retry.
            for (const id of freshIds) removePendingDelete(id);
            addToast({
              tone: "error",
              title: t("toast.bulkFailed", { action: label }),
              description: localizedErrorMessage(err, t),
              key: "bulk-delete",
            });
          }
        })();
      }, UNDO_TOAST_TIMEOUT_MS);
      addToast({
        tone: "info",
        title: t("toast.tasksDeleted", { count: fresh.length }),
        description: t("toast.undoHint"),
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        key: "bulk-soft-delete",
        action: {
          label: t("toast.undo"),
          onClick: () => {
            window.clearTimeout(commitTimer);
            // Restore each freshly-hidden task.
            for (const id of freshIds) removePendingDelete(id);
          },
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
        addToast({
          tone: "success",
          title: t("toast.bulkComplete", { action: label, done, total }),
          key: "bulk-delete-files",
        });
      } catch (error) {
        addToast({
          tone: "error",
          title: t("toast.bulkFailed", { action: label }),
          description: error instanceof Error ? error.message : String(error),
          key: "bulk-delete-files",
        });
      }
      clearSelectedIds();
    })();
  }, [addToast, bulkDeleteFilesTargets, clearSelectedIds, t]);

  const openNewDownload = useCallback((initialState?: NewDownloadInitialState) => {
    if (initialState) {
      setNewDownloadInitialState(initialState);
    } else {
      setNewDownloadInitialState(null);
    }
    setNewDownloadOpen(true);
  }, []);

  const applyClipboardDownload = useCallback((sourceId: string, urls: string[]) => {
    if (urls.length === 0) return;
    setNewDownloadInitialState({
      sourceId,
      url: urls.length === 1 ? urls[0] : undefined,
      batchInput: urls.length > 1 ? urls.join("\n") : undefined,
    });
    setNewDownloadOpen(true);
  }, []);

  const pasteAndCreate = useCallback(() => {
    void (async () => {
      const text = await navigator.clipboard?.readText().catch(() => null);
      if (text?.trim()) {
        const urls = text
          .split(/\s+/)
          .map((s) => s.trim())
          .filter(Boolean);
        applyClipboardDownload("clipboard", urls);
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
          });
        }
        return;
      }
      if (action === "check_url") {
        selectTask(task.id);
        setDetailOpen(true);
        addToast({
          tone: "info",
          title: t("recovery.checkUrlToast"),
          description: sanitizeUrlForDisplay(task.url),
        });
        return;
      }

      if (action === "configure_ffmpeg") {
        // Open Settings → External tools. The user can configure or detect
        // ffmpeg, then retry the task from its row context menu.
        setNav("settings");
        addToast({
          tone: "info",
          title: t("settings.ffmpegPath.label"),
          description: t("settings.ffmpegPath.notDetected"),
        });
        return;
      }

      if (action === "choose_another_name") {
        setAttentionRequest({ task, action });
        return;
      }

      if (action === "choose_another_folder") {
        const saveDir = await openDirectoryPicker();
        if (!saveDir) return;
        submitAttentionResolution(task, action, { saveDir });
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

  useEffect(() => {
    void getPlatform().then(setPlatform);
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
  }, [setSettings, setSettingsError, setSettingsLoading]);

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

  useEffect(() => {
    let cancelled = false;
    let unlistenClipboard: (() => void) | undefined;

    void (async () => {
      unlistenClipboard = await onClipboardLinkDetected((payload) => {
        if (payload.urls.length === 0) return;
        if (newDownloadOpen && newDownloadDraftDirty) {
          addToast({
            tone: "info",
            title:
              payload.urls.length > 1
                ? t("toast.clipboardLinksDetected", { count: payload.urls.length })
                : t("toast.clipboardLinkDetected"),
            description:
              payload.urls.length > 1
                ? t("toast.clipboardLinksDetectedDescription", {
                    count: payload.urls.length,
                  })
                : sanitizeUrlForDisplay(payload.primaryUrl),
            action: {
              label: t("toast.useClipboardLink"),
              onClick: () => applyClipboardDownload(payload.id, payload.urls),
            },
          });
          return;
        }
        applyClipboardDownload(payload.id, payload.urls);
      });
      if (cancelled) unlistenClipboard?.();
    })();

    return () => {
      cancelled = true;
      unlistenClipboard?.();
    };
  }, [addToast, applyClipboardDownload, newDownloadDraftDirty, newDownloadOpen, t]);

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
    const accent = settings?.accentColor ?? "blue";
    document.documentElement.dataset.accent = accent;
    try {
      localStorage.setItem("vibe-accent", accent);
    } catch {
      // localStorage unavailable; skip persistence
    }
  }, [settings?.accentColor]);

  const [dropActive, setDropActive] = useState(false);

  const applyDroppedFile = useCallback((initialState: NewDownloadInitialState) => {
    setNewDownloadInitialState(initialState);
    setNewDownloadOpen(true);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlistenFileDrop: (() => void) | undefined;

    void (async () => {
      unlistenFileDrop = await onFileDrop(
        async (paths) => {
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
            if (newDownloadOpen && newDownloadDraftDirty) {
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
        (state: FileDropDragState) => setDropActive(state.active),
      );
      if (cancelled) unlistenFileDrop?.();
    })();

    return () => {
      cancelled = true;
      unlistenFileDrop?.();
    };
  }, [addToast, applyDroppedFile, newDownloadDraftDirty, newDownloadOpen, t]);

  useEffect(() => {
    const NAV_KEYS: Record<string, "all" | "downloading" | "paused" | "completed" | "failed"> = {
      "1": "all",
      "2": "downloading",
      "3": "paused",
      "4": "completed",
      "5": "failed",
    };

    const onKeyDown = (event: KeyboardEvent) => {
      // IME composition guard: when a CJK input method is composing (e.g. typing
      // pinyin), keystrokes should go to the IME, not trigger app shortcuts.
      if (event.isComposing) return;
      const target = event.target as HTMLElement;
      const isInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable;

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

      // ── Non-input shortcuts ──

      if (!isInput) {
        if (event.key === "?") {
          event.preventDefault();
          setShortcutPanelOpen((prev) => !prev);
          return;
        }

        // Navigation: Mod+1~5
        const navTarget = NAV_KEYS[event.key];
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

        // Retry failed task: Mod+R
        if (matchesShortcut(event, "mod+r", platform) && selected) {
          if (selected.status === "failed" || selected.status === "needs_attention") {
            event.preventDefault();
            retry(selected);
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

        // Select all loaded: Mod+A
        if (matchesShortcut(event, "mod+a", platform)) {
          event.preventDefault();
          // taskIds is the cursor-paginated loaded subset, not the full
          // filtered result. Select what's loaded and, when more pages
          // remain, honestly tell the user so the scope is clear.
          const loaded = taskIds.filter((id) => !pendingDeleteIds.includes(id));
          setSelectedIds(loaded);
          if (hasMoreTasks && loaded.length < totalTasks) {
            addToast({
              tone: "info",
              title: t("toast.selectedLoadedOnly", { loaded: loaded.length, total: totalTasks }),
            });
          }
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
    addToast,
    clearSelectedIds,
    detailOpen,
    handleReorder,
    hasMoreTasks,
    openFile,
    openFolder,
    openNewDownload,
    pendingDeleteIds,
    platform,
    retry,
    selectTask,
    selected,
    selectedId,
    setSelectedIds,
    setDetailOpen,
    setDeleteFilesTarget,
    setNav,
    softDelete,
    taskIds,
    t,
    totalTasks,
  ]);

  // ── Global native context-menu suppression ──
  // WebView2/WebKit ship a default context menu (Inspect, Properties, Copy,
  // etc.) on any element that doesn't preventDefault. Radix-owned triggers
  // already call preventDefault when they open our custom menu; for every
  // other surface (sidebar, status bar, title bar, settings, about, blank
  // gaps) we suppress the native menu here. Custom region menus added in
  // later phases will still work because they render via Radix, which calls
  // preventDefault on the trigger before this listener runs.
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      event.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu, { capture: true });
    return () => window.removeEventListener("contextmenu", onContextMenu, { capture: true } as AddEventListenerOptions);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <TitleBar
        platform={platform}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDownload={() => openNewDownload()}
        onOpenShortcuts={() => setShortcutPanelOpen(true)}
      />
      <CommandBar
        platform={platform}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDownload={() => openNewDownload()}
        inputRef={searchInputRef}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col md:flex-row">
        <Sidebar onNewDownload={() => openNewDownload()} />
        <main className="order-1 flex min-h-0 min-w-0 flex-1 md:order-none">
          <h1 className="sr-only">{t("app.name")}</h1>
          <TaskList
            onToggleTransfer={toggleTransfer}
            onRetry={retry}
            onFinishLiveRecording={finishRecording}
            onOpenFile={openFile}
            onOpenFolder={openFolder}
            onResolveAttention={resolveAttention}
            onDelete={(task) => softDelete(task)}
            onDeleteFiles={(task) => setDeleteFilesTarget(task)}
            onNewDownload={() => openNewDownload()}
            onBulkPause={bulkPause}
            onBulkResume={bulkResume}
            onBulkRetry={bulkRetry}
            onBulkDelete={bulkDelete}
            onBulkDeleteFiles={bulkDeleteFiles}
            onBulkOpenFolder={bulkOpenFolder}
            onBulkExport={bulkExport}
            onOpenOnboarding={() => setOnboardingOpen(true)}
            onCopyUrl={copyTaskUrl}
            onCopyLocalPath={copyTaskLocalPath}
            onShowDetails={showTaskDetails}
            onPasteAndCreate={pasteAndCreate}
            onRefresh={refreshTaskList}
            onReorder={handleReorder}
          />
          <Suspense fallback={null}>
            <TaskDetails
              task={taskSurfaceActive ? selected : null}
              open={taskSurfaceActive && detailOpen && !!selected}
              onClose={() => {
                setDetailOpen(false);
                const focusId = selectedId;
                if (focusId) {
                  requestAnimationFrame(() => {
                    document.getElementById(`task-option-${focusId}`)?.focus();
                  });
                }
              }}
              onResolveAttention={resolveAttention}
            />
          </Suspense>
        </main>
      </div>
      <StatusBar
        className="flex"
        platform={platform}
        onOpenShortcuts={() => setShortcutPanelOpen(true)}
        onOpenAbout={() => setNav("about")}
      />
      <ToastViewport />
      {paletteOpen ? (
        <Suspense fallback={null}>
          <Palette
            open={paletteOpen}
            onOpenChange={setPaletteOpen}
            platform={platform}
            selectedTask={taskSurfaceActive ? selected : null}
            onNewDownload={() => openNewDownload()}
            onStart={() => {
              if (selected) void runTaskAction(() => resumeTask(selected.id), selected.id);
            }}
            onPause={() => {
              if (selected) void runTaskAction(() => pauseTask(selected.id), selected.id);
            }}
            onDelete={() => {
              if (selected) softDelete(selected);
            }}
            onRetry={() => {
              if (selected) retry(selected);
            }}
            onOpenFile={() => {
              if (selected) openFile(selected);
            }}
            onOpenFolder={() => {
              if (selected) openFolder(selected);
            }}
            onBulkPause={bulkPause}
            onBulkResume={bulkResume}
            onBulkRetry={bulkRetry}
            onBulkDelete={bulkDelete}
            onBulkOpenFolder={bulkOpenFolder}
            onSetNav={(nextNav) => {
              useTaskUIStore.getState().setNav(nextNav);
              if (nextNav === "settings") {
                setDetailOpen(false);
              }
            }}
          />
        </Suspense>
      ) : null}
      {newDownloadOpen ? (
        <Suspense fallback={null}>
          <NewDownloadDialog
            open={newDownloadOpen}
            onOpenChange={(open) => {
              setNewDownloadOpen(open);
              if (!open) setNewDownloadDraftDirty(false);
            }}
            initialSourceId={newDownloadInitialState?.sourceId}
            initialUrl={newDownloadInitialState?.url}
            initialBatchInput={newDownloadInitialState?.batchInput}
            onDraftStateChange={setNewDownloadDraftDirty}
            onCreated={(task) => {
              useTaskDataStore.getState().upsertTask(task);
              selectTask(task.id);
            }}
          />
        </Suspense>
      ) : null}
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
                void runTaskAction(() => deleteTask(target.id, true));
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
              if (request) {
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

  const titleKey = `completionDialog.${request.action}Title` as const;
  const descriptionKey = `completionDialog.${request.action}Description` as const;
  const confirmKey =
    `completionDialog.confirm${request.action === "exit_app" ? "Exit" : request.action === "shutdown" ? "Shutdown" : request.action === "sleep" ? "Sleep" : request.action === "hibernate" ? "Hibernate" : request.action === "lock_screen" ? "Lock" : "Run"}` as const;

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
            <div className="h-1.5 overflow-hidden rounded-full bg-surface-raised">
              <div
                className="h-full rounded-full bg-accent-primary transition-[width] duration-300"
                style={{
                  width: `${Math.max(0, Math.min(100, (remaining / Math.max(1, request.countdownSeconds)) * 100))}%`,
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
