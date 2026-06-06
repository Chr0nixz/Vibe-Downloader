import { lazy, Suspense, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { CommandBar } from "@/components/shell/CommandBar";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import { TitleBar } from "@/components/shell/TitleBar";
import { TaskList } from "@/components/tasks/TaskList";
import { ToastViewport } from "@/components/ui/toast";
import { readShellLayout } from "@/hooks/use-shell-layout";
import { useActiveDownloadSync } from "@/hooks/use-active-download-sync";
import { useTaskEvents } from "@/hooks/use-task-events";
import {
  getPlatform,
  trafficLightsInsetPx,
  type Platform,
} from "@/lib/platform";
import {
  deleteTask,
  getSettings,
  listTasks,
  onSettingsChanged,
  openTaskFile,
  openTaskFolder,
  pauseTask,
  retryTask,
  resumeTask,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { useTaskStore } from "@/stores/task-store";
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
  const [newDownloadOpen, setNewDownloadOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Task | null>(null);

  const tasks = useTaskStore((s) => s.tasks);
  const selectedId = useTaskStore((s) => s.selectedId);
  const detailOpen = useTaskStore((s) => s.detailOpen);
  const setTasks = useTaskStore((s) => s.setTasks);
  const setLoading = useTaskStore((s) => s.setLoading);
  const setError = useTaskStore((s) => s.setError);
  const selectTask = useTaskStore((s) => s.selectTask);
  const setDetailOpen = useTaskStore((s) => s.setDetailOpen);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setSettingsLoading = useSettingsStore((s) => s.setLoading);
  const setSettingsError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);

  const selected = tasks.find((t) => t.id === selectedId) ?? null;

  async function refreshTasks(selectId?: string) {
    const data = await listTasks();
    setTasks(data);
    if (selectId) {
      selectTask(selectId);
    } else if (data.length > 0 && (!selectedId || !data.some((task) => task.id === selectedId))) {
      selectTask(data[0].id);
    } else if (data.length === 0) {
      selectTask(null);
    }
  }

  async function runTaskAction(action: () => Promise<Task | void>, selectId?: string) {
    try {
      await action();
      setError(null);
      await refreshTasks(selectId);
    } catch (err) {
      const message = String(err);
      setError(message);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: message,
      });
    }
  }

  function toggleTransfer(task: Task) {
    if (task.status === "downloading" || task.status === "retrying" || task.status === "queued") {
      void runTaskAction(() => pauseTask(task.id), task.id);
    } else if (task.status !== "completed" && task.status !== "needs_attention") {
      void runTaskAction(() => resumeTask(task.id), task.id);
    }
  }

  function retry(task: Task) {
    void runTaskAction(() => retryTask(task.id), task.id);
  }

  function openFile(task: Task) {
    void runTaskAction(() => openTaskFile(task.id), task.id);
  }

  function openFolder(task: Task) {
    void runTaskAction(() => openTaskFolder(task.id), task.id);
  }

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
  useActiveDownloadSync();

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const data = await listTasks();
        if (!cancelled) {
          setTasks(data);
          const currentSelectedId = useTaskStore.getState().selectedId;
          if (data.length > 0 && !currentSelectedId) {
            selectTask(data[0].id);
            if (readShellLayout() === "wide") {
              setDetailOpen(true);
            }
          }
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [selectTask, setDetailOpen, setError, setLoading, setTasks]);

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
        if (!cancelled) setSettingsError(String(err));
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
    const onKeyDown = (event: KeyboardEvent) => {
      if (matchesShortcut(event, "mod+k", platform)) {
        event.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [platform]);

  return (
    <div className="flex h-full flex-col">
      <TitleBar platform={platform} />
      <CommandBar
        platform={platform}
        selectedTask={selected}
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
      <div className="flex min-h-0 min-w-0 flex-1">
        <Sidebar />
        <TaskList
          onToggleTransfer={toggleTransfer}
          onRetry={retry}
          onOpenFile={openFile}
          onOpenFolder={openFolder}
        />
        <Suspense fallback={null}>
          <TaskDetails
            task={selected}
            open={detailOpen && !!selected}
            onClose={() => {
              setDetailOpen(false);
              const focusId = selectedId;
              if (focusId) {
                requestAnimationFrame(() => {
                  document.getElementById(`task-option-${focusId}`)?.focus();
                });
              }
            }}
          />
        </Suspense>
      </div>
      <StatusBar />
      <ToastViewport />
      {paletteOpen ? (
        <Suspense fallback={null}>
          <Palette open={paletteOpen} onOpenChange={setPaletteOpen} />
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
    </div>
  );
}
