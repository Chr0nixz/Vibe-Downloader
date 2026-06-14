import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { CommandBar } from "@/components/shell/CommandBar";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import { TitleBar } from "@/components/shell/TitleBar";
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
import type { AttentionDialogRequest } from "@/components/shell/ResolveAttentionDialog";
import { useTaskEvents } from "@/hooks/use-task-events";
import { createLogger } from "@/lib/logger";
import { errorMessage } from "@/lib/errors";
import type {
  CompletionActionRequestedPayload,
  RecoveryAction,
  ResolveTaskAttentionInput,
} from "@/generated/bindings";
import {
  getPlatform,
  trafficLightsInsetPx,
  type Platform,
} from "@/lib/platform";
import { cn, sanitizeUrlForDisplay } from "@/lib/utils";

const log = createLogger("app-shell");
import {
  deleteTask,
  getSettings,
  listTasksCursor,
  onCompletionActionRequested,
  onClipboardLinkDetected,
  onSettingsChanged,
  onTrayNewDownloadRequested,
  onTraySettingsRequested,
  openTaskFile,
  openTaskFolder,
  openDirectoryPicker,
  requestSystemShutdown,
  finishLiveRecording,
  pauseTask,
  resolveTaskAttention,
  retryTask,
  resumeTask,
  runTrayMenuAction,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { taskCursorInput, useTaskStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
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
  const [newDownloadInitialState, setNewDownloadInitialState] =
    useState<NewDownloadInitialState | null>(null);
  const [newDownloadDraftDirty, setNewDownloadDraftDirty] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Task | null>(null);
  const [bulkDeleteTargets, setBulkDeleteTargets] = useState<Task[]>([]);
  const [attentionRequest, setAttentionRequest] =
    useState<AttentionDialogRequest | null>(null);
  const [completionActionRequest, setCompletionActionRequest] =
    useState<CompletionActionRequestedPayload | null>(null);

  const tasks = useTaskStore((s) => s.tasks);
  const selectedId = useTaskStore((s) => s.selectedId);
  const nav = useTaskStore((s) => s.nav);
  const detailOpen = useTaskStore((s) => s.detailOpen);
  const setTaskCursorPage = useTaskStore((s) => s.setTaskCursorPage);
  const upsertTask = useTaskStore((s) => s.upsertTask);
  const setError = useTaskStore((s) => s.setError);
  const selectTask = useTaskStore((s) => s.selectTask);
  const clearSelectedIds = useTaskStore((s) => s.clearSelectedIds);
  const setDetailOpen = useTaskStore((s) => s.setDetailOpen);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setSettingsLoading = useSettingsStore((s) => s.setLoading);
  const setSettingsError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);
  const updateToast = useToastStore((s) => s.updateToast);
  const dismissToast = useToastStore((s) => s.dismissToast);
  const taskLoading = useTaskStore((s) => s.loading);

  /* ── Splash overlay ── */
  const initialLoadDoneRef = useRef(false);
  const [splashVisible, setSplashVisible] = useState(true);
  const [splashDismissing, setSplashDismissing] = useState(false);

  const dismissSplash = useCallback(() => {
    if (initialLoadDoneRef.current) return;
    initialLoadDoneRef.current = true;
    const fadeTimer = setTimeout(() => setSplashDismissing(true), 200);
    const removeTimer = setTimeout(() => setSplashVisible(false), 420);
    return () => {
      clearTimeout(fadeTimer);
      clearTimeout(removeTimer);
    };
  }, []);

  useEffect(() => {
    if (!taskLoading) dismissSplash();
  }, [taskLoading, dismissSplash]);

  // Safety timeout: force-dismiss splash after 3 seconds regardless of loading state
  useEffect(() => {
    const timer = setTimeout(() => {
      if (!initialLoadDoneRef.current) {
        dismissSplash();
        useTaskStore.getState().setLoading(false);
      }
    }, 3000);
    return () => clearTimeout(timer);
  }, [dismissSplash]);

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

  const finishRecording = useCallback((task: Task) => {
    void runTaskAction(() => finishLiveRecording(task.id), task.id);
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
    label: string,
  ) => {
    if (selectedTasks.length === 0) return;
    const total = selectedTasks.length;
    const toastId = addToast({
      tone: "info",
      title: t("toast.bulkProgress", { action: label, done: 0, total }),
    });
    let done = 0;
    for (const task of selectedTasks) {
      await runTaskAction(() => action(task), task.id);
      done++;
      updateToast(toastId, {
        description: t("toast.bulkProgress", { action: label, done, total }),
      });
    }
    dismissToast(toastId);
    addToast({
      tone: "success",
      title: t("toast.bulkComplete", { action: label, done, total }),
    });
  }, [addToast, dismissToast, runTaskAction, t, updateToast]);

  const bulkPause = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) =>
        task.status === "downloading" ||
        task.status === "retrying" ||
        task.status === "queued",
      ),
      (task) => pauseTask(task.id),
      t("taskList.bulkPause"),
    );
  }, [runBulkTaskAction, t]);

  const bulkResume = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) =>
        task.status === "paused" ||
        task.status === "failed" ||
        task.status === "waiting_network",
      ),
      (task) => resumeTask(task.id),
      t("taskList.bulkResume"),
    );
  }, [runBulkTaskAction, t]);

  const bulkRetry = useCallback((selectedTasks: Task[]) => {
    void runBulkTaskAction(
      selectedTasks.filter((task) => task.status !== "completed"),
      (task) => retryTask(task.id),
      t("taskList.bulkRetry"),
    );
  }, [runBulkTaskAction, t]);

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
    if (targets.length === 0) return;
    void (async () => {
      const total = targets.length;
      const label = t("taskList.bulkDelete", { count: total });
      const toastId = addToast({
        tone: "info",
        title: t("toast.bulkProgress", { action: label, done: 0, total }),
      });
      let done = 0;
      for (const task of targets) {
        await runTaskAction(() => deleteTask(task.id, deleteFile));
        done++;
        updateToast(toastId, {
          description: t("toast.bulkProgress", { action: label, done, total }),
        });
      }
      dismissToast(toastId);
      addToast({
        tone: "success",
        title: t("toast.bulkComplete", { action: label, done, total }),
      });
      clearSelectedIds();
    })();
  }, [addToast, bulkDeleteTargets, clearSelectedIds, dismissToast, runTaskAction, t, updateToast]);

  const openNewDownload = useCallback((initialState?: NewDownloadInitialState) => {
    if (initialState) {
      setNewDownloadInitialState(initialState);
    } else {
      setNewDownloadInitialState(null);
    }
    setNewDownloadOpen(true);
  }, []);

  const applyClipboardDownload = useCallback(
    (sourceId: string, urls: string[]) => {
      if (urls.length === 0) return;
      setNewDownloadInitialState({
        sourceId,
        url: urls.length === 1 ? urls[0] : undefined,
        batchInput: urls.length > 1 ? urls.join("\n") : undefined,
      });
      setNewDownloadOpen(true);
    },
    [],
  );

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

  const runCompletionAction = useCallback(
    async (request: CompletionActionRequestedPayload) => {
      setCompletionActionRequest(null);
      try {
        if (request.action === "exit_app") {
          await runTrayMenuAction("quit");
        } else if (request.action === "shutdown") {
          await requestSystemShutdown();
        }
      } catch (err) {
        const message = errorMessage(err);
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
        openNewDownload();
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
  }, [
    addToast,
    applyClipboardDownload,
    newDownloadDraftDirty,
    newDownloadOpen,
    t,
  ]);

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
        openNewDownload();
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
  }, [openNewDownload, platform, setDetailOpen]);

  return (
    <div className="flex h-full flex-col">
      <TitleBar platform={platform} />
      <CommandBar
        platform={platform}
        selectedTask={taskSurfaceActive ? selected : null}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewDownload={() => openNewDownload()}
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
            onFinishLiveRecording={finishRecording}
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
            onNewDownload={() => openNewDownload()}
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
            onOpenChange={(open) => {
              setNewDownloadOpen(open);
              if (!open) setNewDownloadDraftDirty(false);
            }}
            initialSourceId={newDownloadInitialState?.sourceId}
            initialUrl={newDownloadInitialState?.url}
            initialBatchInput={newDownloadInitialState?.batchInput}
            onDraftStateChange={setNewDownloadDraftDirty}
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
          <ShortcutPanel
            open={shortcutPanelOpen}
            onOpenChange={setShortcutPanelOpen}
            platform={platform}
          />
        </Suspense>
      ) : null}
      {splashVisible ? (
        <div
          className={cn(
            "splash-overlay fixed inset-0 z-50 flex items-center justify-center bg-surface-root",
            splashDismissing && "pointer-events-none",
          )}
          style={
            splashDismissing
              ? { animation: "splash-fade-out 220ms ease-out forwards" }
              : undefined
          }
          role="status"
          aria-label={t("app.name")}
        >
          <div className="flex flex-col items-center gap-4">
            <img
              src="/logo-64.png"
              alt=""
              width={72}
              height={72}
              className="select-none"
              draggable={false}
            />
            <span className="text-lg font-semibold tracking-wide text-text-primary">
              {t("app.name")}
            </span>
            <div
              className="splash-indicator mt-1 h-2 w-2 rounded-full bg-accent-primary"
              style={{ animation: "splash-breathe 1.5s ease-in-out infinite" }}
            />
          </div>
        </div>
      ) : null}
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
  const isExit = request.action === "exit_app";
  const isShutdown = request.action === "shutdown";

  useEffect(() => {
    setRemaining(request.countdownSeconds);
  }, [request]);

  useEffect(() => {
    if (!open || !isExit) return;
    if (remaining <= 0) {
      void onRun(request);
      return;
    }
    const timer = window.setTimeout(() => {
      setRemaining((value) => Math.max(0, value - 1));
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [isExit, onRun, open, remaining, request]);

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen) onCancel();
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {isShutdown
              ? t("completionDialog.shutdownTitle")
              : t("completionDialog.exitTitle")}
          </DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <DialogDescription>
            {isShutdown
              ? t("completionDialog.shutdownDescription")
              : t("completionDialog.exitDescription", { seconds: remaining })}
          </DialogDescription>
          {isExit ? (
            <div className="h-1.5 overflow-hidden rounded-full bg-surface-raised">
              <div
                className="h-full rounded-full bg-accent-primary transition-[width] duration-300"
                style={{
                  width: `${Math.max(
                    0,
                    Math.min(100, (remaining / Math.max(1, request.countdownSeconds)) * 100),
                  )}%`,
                }}
              />
            </div>
          ) : null}
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {t("completionDialog.cancel")}
          </Button>
          <Button
            variant={isShutdown ? "danger" : "default"}
            onClick={() => void onRun(request)}
          >
            {isShutdown
              ? t("completionDialog.confirmShutdown")
              : t("completionDialog.exitNow")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
