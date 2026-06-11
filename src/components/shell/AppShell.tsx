import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { CommandBar } from "@/components/shell/CommandBar";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import { TitleBar } from "@/components/shell/TitleBar";
import { TaskList } from "@/components/tasks/TaskList";
import { ToastViewport } from "@/components/ui/toast";
import type { AttentionDialogRequest } from "@/components/shell/ResolveAttentionDialog";
import { readShellLayout } from "@/hooks/use-shell-layout";
import { useTaskEvents } from "@/hooks/use-task-events";
import { createLogger } from "@/lib/logger";
import { errorMessage } from "@/lib/errors";
import type { RecoveryAction, ResolveTaskAttentionInput } from "@/generated/bindings";
import {
  getPlatform,
  trafficLightsInsetPx,
  type Platform,
} from "@/lib/platform";
import { sanitizeUrlForDisplay } from "@/lib/utils";

const log = createLogger("app-shell");
import {
  deleteTask,
  getSettings,
  listTasksCursor,
  onSettingsChanged,
  onTrayNewDownloadRequested,
  onTraySettingsRequested,
  openTaskFile,
  openTaskFolder,
  openDirectoryPicker,
  pauseTask,
  resolveTaskAttention,
  retryTask,
  resumeTask,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { taskCursorInput, useTaskStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

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

function matchesShortcut(
  event: KeyboardEvent,
  shortcut: string,
  platform: Platform,
): boolean {
  const parts = shortcut.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  if (parts.includes("mod")) {
    const modOk = platform === "macos" ? event.metaKey : event.ctrlKey;
    if (!modOk) return false;
  }
  return event.key.toLowerCase() === key;
}

export function AppShell() {
  const { t } = useTranslation();
  const [platform, setPlatform] = useState<Platform>("unknown");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutPanelOpen, setShortcutPanelOpen] = useState(false);
  const [newDownloadOpen, setNewDownloadOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Task | null>(null);
  const [bulkDeleteTargets, setBulkDeleteTargets] = useState<Task[]>([]);
  const [attentionRequest, setAttentionRequest] =
    useState<AttentionDialogRequest | null>(null);

  const tasks = useTaskStore((s) => s.tasks);
  const selectedId = useTaskStore((s) => s.selectedId);
  const nav = useTaskStore((s) => s.nav);
  const detailOpen = useTaskStore((s) => s.detailOpen);
  const setTaskCursorPage = useTaskStore((s) => s.setTaskCursorPage);
  const upsertTask = useTaskStore((s) => s.upsertTask);
  const setLoading = useTaskStore((s) => s.setLoading);
  const setError = useTaskStore((s) => s.setError);
  const selectTask = useTaskStore((s) => s.selectTask);
  const clearSelectedIds = useTaskStore((s) => s.clearSelectedIds);
  const setDetailOpen = useTaskStore((s) => s.setDetailOpen);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setSettingsLoading = useSettingsStore((s) => s.setLoading);
  const setSettingsError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);

  const selected = tasks.find((t) => t.id === selectedId) ?? null;
  const taskSurfaceActive = nav !== "settings";

  const refreshTasks = useCallback(async (selectId?: string) => {
    const page = await listTasksCursor(taskCursorInput(null));
    const data = page.items;
    setTaskCursorPage(data, page.totalEstimate, page.nextCursor, page.filterOptions);
    if (selectId) {
      selectTask(selectId);
    } else {
      const currentSelectedId = useTaskStore.getState().selectedId;
      if (
        data.length > 0 &&
        (!currentSelectedId || !data.some((task) => task.id === currentSelectedId))
      ) {
        selectTask(data[0].id);
      } else if (data.length === 0) {
        selectTask(null);
      }
    }
  }, [selectTask, setTaskCursorPage]);

  const runTaskAction = useCallback(async (action: () => Promise<Task | void>, selectId?: string) => {
    try {
      const result = await action();
      setError(null);
      if (result) {
        upsertTask(result);
        if (selectId) selectTask(selectId);
      } else {
        await refreshTasks(selectId);
      }
    } catch (err) {
      const message = errorMessage(err);
      log.error("task action failed", err);
      setError(message);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: message,
      });
    }
  }, [addToast, refreshTasks, selectTask, setError, t, upsertTask]);

  const toggleTransfer = useCallback((task: Task) => {
    if (task.status === "downloading" || task.status === "retrying" || task.status === "queued") {
      void runTaskAction(() => pauseTask(task.id), task.id);
    } else if (task.status !== "completed" && task.status !== "needs_attention") {
      void runTaskAction(() => resumeTask(task.id), task.id);
    }
  }, [runTaskAction]);

  const retry = useCallback((task: Task) => {
    void runTaskAction(() => retryTask(task.id), task.id);
  }, [runTaskAction]);

  const openFile = useCallback((task: Task) => {
    void runTaskAction(() => openTaskFile(task.id), task.id);
  }, [runTaskAction]);

  const openFolder = useCallback((task: Task) => {
    void runTaskAction(() => openTaskFolder(task.id), task.id);
  }, [runTaskAction]);

  const runBulkTaskAction = useCallback(async (
    selectedTasks: Task[],
    action: (task: Task) => Promise<Task | void>,
  ) => {
    for (const task of selectedTasks) {
      await runTaskAction(() => action(task), task.id);
    }
  }, [runTaskAction]);

  const bulkPause = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) =>
        task.status === "downloading" ||
        task.status === "retrying" ||
        task.status === "queued",
      ),
      (task) => pauseTask(task.id),
    );
  }, [runBulkTaskAction]);

  const bulkResume = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) =>
        task.status === "paused" ||
        task.status === "failed" ||
        task.status === "waiting_network",
      ),
      (task) => resumeTask(task.id),
    );
  }, [runBulkTaskAction]);

  const bulkRetry = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) => task.status !== "completed"),
      (task) => retryTask(task.id),
    );
  }, [runBulkTaskAction]);

  const bulkOpenFolder = useCallback((selectedTasks: Task[]) => {
    const first = selectedTasks[0];
    if (first) openFolder(first);
  }, [openFolder]);

  const bulkDelete = useCallback((selectedTasks: Task[]) => {
    if (selectedTasks.length === 0) return;
    setBulkDeleteTargets(selectedTasks);
  }, []);

  const confirmBulkDelete = useCallback((deleteFile: boolean) => {
    const targets = bulkDeleteTargets;
    setBulkDeleteTargets([]);
    void (async () => {
      for (const task of targets) {
        await runTaskAction(() => deleteTask(task.id, deleteFile));
      }
      clearSelectedIds();
    })();
  }, [bulkDeleteTargets, clearSelectedIds, runTaskAction]);

  const submitAttentionResolution = useCallback((
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
  }, [runTaskAction]);

  const resolveAttention = useCallback(async (task: Task, action: RecoveryAction) => {
    if (action === "open_folder" || action === "free_disk_space") {
      openFolder(task);
      if (action === "free_disk_space") {
        addToast({
          tone: "info",
          title: t("recovery.freeDiskSpaceToast"),
          description: task.saveDir,
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
  }, [addToast, openFolder, selectTask, setDetailOpen, submitAttentionResolution, t]);

  useEffect(() => {
    void getPlatform().then(setPlatform);
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-platform", platform);
    document.documentElement.style.setProperty(
      "--traffic-lights-inset",
      `${trafficLightsInsetPx(platform)}px`,
    );
  }, [platform]);

  useTaskEvents();

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        await refreshTasks();
        if (!cancelled) {
          const selectedId = useTaskStore.getState().selectedId;
          if (selectedId && readShellLayout() === "wide") {
            setDetailOpen(true);
          }
        }
      } catch (err) {
        if (!cancelled) {
          log.error("initial task load failed", err);
          setError(errorMessage(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [refreshTasks, setDetailOpen, setError, setLoading]);

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
          setSettingsError(errorMessage(err));
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
        setNewDownloadOpen(true);
      });
      unlistenSettings = await onTraySettingsRequested(() => {
        useTaskStore.getState().setNav("settings");
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
  }, [setDetailOpen]);

  useEffect(() => {
    document.documentElement.dataset.fontFamily =
      settings?.fontFamily ?? "source_han_sans_sc";
  }, [settings?.fontFamily]);

  useEffect(() => {
    document.documentElement.dataset.accent =
      settings?.accentColor ?? "blue";
  }, [settings?.accentColor]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      const isInput =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      if (matchesShortcut(event, "mod+k", platform)) {
        event.preventDefault();
        setPaletteOpen(true);
        return;
      }

      if (matchesShortcut(event, "mod+/", platform)) {
        event.preventDefault();
        setShortcutPanelOpen((prev) => !prev);
        return;
      }

      if (!isInput) {
        if (event.key === "?") {
          event.preventDefault();
          setShortcutPanelOpen((prev) => !prev);
          return;
        }
      }

      if (matchesShortcut(event, "mod+n", platform)) {
        event.preventDefault();
        setNewDownloadOpen(true);
        return;
      }

      if (matchesShortcut(event, "mod+,", platform)) {
        event.preventDefault();
        useTaskStore.getState().setNav("settings");
        setDetailOpen(false);
        return;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [platform, setDetailOpen]);

  return (
    <div className="flex h-full flex-col">
      <TitleBar platform={platform} />
      <CommandBar
        platform={platform}
        selectedTask={taskSurfaceActive ? selected : null}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDownload={() => setNewDownloadOpen(true)}
        onStart={() => {
          if (selected) void runTaskAction(() => resumeTask(selected.id), selected.id);
        }}
        onPause={() => {
          if (selected) void runTaskAction(() => pauseTask(selected.id), selected.id);
        }}
        onDelete={() => {
          if (selected) setDeleteTarget(selected);
        }}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col md:flex-row">
        <Sidebar />
        <main className="order-1 flex min-h-0 min-w-0 flex-1 md:order-none">
          <h1 className="sr-only">{t("app.name")}</h1>
          <TaskList
            onToggleTransfer={toggleTransfer}
            onRetry={retry}
            onOpenFile={openFile}
            onOpenFolder={openFolder}
            onResolveAttention={resolveAttention}
            onBulkPause={bulkPause}
            onBulkResume={bulkResume}
            onBulkRetry={bulkRetry}
            onBulkDelete={bulkDelete}
            onBulkOpenFolder={bulkOpenFolder}
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
        <StatusBar className="md:hidden" />
      </div>
      <StatusBar className="hidden md:flex" />
      <ToastViewport />
      {paletteOpen ? (
        <Suspense fallback={null}>
          <Palette
            open={paletteOpen}
            onOpenChange={setPaletteOpen}
            platform={platform}
            selectedTask={taskSurfaceActive ? selected : null}
            onNewDownload={() => setNewDownloadOpen(true)}
            onStart={() => {
              if (selected) void runTaskAction(() => resumeTask(selected.id), selected.id);
            }}
            onPause={() => {
              if (selected) void runTaskAction(() => pauseTask(selected.id), selected.id);
            }}
            onDelete={() => {
              if (selected) setDeleteTarget(selected);
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
              useTaskStore.getState().setNav(nextNav);
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
            onOpenChange={setNewDownloadOpen}
            onCreated={(task) => {
              useTaskStore.getState().upsertTask(task);
              selectTask(task.id);
            }}
          />
        </Suspense>
      ) : null}
      {deleteTarget ? (
        <Suspense fallback={null}>
          <DeleteTaskDialog
            task={deleteTarget}
            open={!!deleteTarget}
            onOpenChange={(open) => {
              if (!open) setDeleteTarget(null);
            }}
            onDelete={(deleteFile) => {
              const target = deleteTarget;
              setDeleteTarget(null);
              if (target) {
                void runTaskAction(() => deleteTask(target.id, deleteFile));
              }
            }}
          />
        </Suspense>
      ) : null}
      {bulkDeleteTargets.length > 0 ? (
        <Suspense fallback={null}>
          <BulkDeleteDialog
            tasks={bulkDeleteTargets}
            open={bulkDeleteTargets.length > 0}
            onOpenChange={(open) => {
              if (!open) setBulkDeleteTargets([]);
            }}
            onDelete={confirmBulkDelete}
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
      {shortcutPanelOpen ? (
        <Suspense fallback={null}>
          <ShortcutPanel
            open={shortcutPanelOpen}
            onOpenChange={setShortcutPanelOpen}
            platform={platform}
          />
        </Suspense>
      ) : null}
    </div>
  );
}
