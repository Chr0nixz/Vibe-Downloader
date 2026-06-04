import { useEffect, useState } from "react";

import { CommandBar } from "@/components/shell/CommandBar";
import { DeleteTaskDialog } from "@/components/shell/DeleteTaskDialog";
import { NewDownloadDialog } from "@/components/shell/NewDownloadDialog";
import { Palette } from "@/components/shell/Palette";
import { Sidebar } from "@/components/shell/Sidebar";
import { StatusBar } from "@/components/shell/StatusBar";
import { TaskDetails } from "@/components/shell/TaskDetails";
import { TitleBar } from "@/components/shell/TitleBar";
import { TaskList } from "@/components/tasks/TaskList";
import {
  getPlatform,
  trafficLightsInsetPx,
  type Platform,
} from "@/lib/platform";

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
import {
  deleteTask,
  listTasks,
  onQueueChanged,
  onTaskProgress,
  openTaskFile,
  openTaskFolder,
  pauseTask,
  retryTask,
  resumeTask,
} from "@/lib/tauri";
import { useTaskStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

export function AppShell() {
  const [platform, setPlatform] = useState<Platform>("unknown");
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [newDownloadOpen, setNewDownloadOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Task | null>(null);

  const tasks = useTaskStore((s) => s.tasks);
  const selectedId = useTaskStore((s) => s.selectedId);
  const detailOpen = useTaskStore((s) => s.detailOpen);
  const setTasks = useTaskStore((s) => s.setTasks);
  const patchTask = useTaskStore((s) => s.patchTask);
  const setLoading = useTaskStore((s) => s.setLoading);
  const setError = useTaskStore((s) => s.setError);
  const selectTask = useTaskStore((s) => s.selectTask);

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
      setError(String(err));
    }
  }

  function toggleTransfer(task: Task) {
    if (task.status === "downloading" || task.status === "retrying") {
      void runTaskAction(() => pauseTask(task.id), task.id);
    } else if (task.status !== "completed") {
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

  useEffect(() => {
    let cancelled = false;
    let unlistenProgress: (() => void) | undefined;
    let unlistenQueue: (() => void) | undefined;

    void (async () => {
      try {
        const data = await listTasks();
        if (!cancelled) {
          setTasks(data);
          if (data.length > 0 && !selectedId) selectTask(data[0].id);
        }
      } catch (err) {
        if (!cancelled) setError(String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }

      unlistenProgress = await onTaskProgress((payload) => {
        patchTask(payload);
      });
      unlistenQueue = await onQueueChanged(async () => {
        try {
          setTasks(await listTasks());
        } catch {
          /* ignore refresh errors */
        }
      });
    })();

    return () => {
      cancelled = true;
      unlistenProgress?.();
      unlistenQueue?.();
    };
  }, [patchTask, selectTask, selectedId, setError, setLoading, setTasks]);

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
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <TaskList
          onToggleTransfer={toggleTransfer}
          onRetry={retry}
          onOpenFile={openFile}
          onOpenFolder={openFolder}
        />
        <TaskDetails task={selected} open={detailOpen && !!selected} />
      </div>
      <StatusBar />
      <Palette open={paletteOpen} onOpenChange={setPaletteOpen} />
      <NewDownloadDialog
        open={newDownloadOpen}
        onOpenChange={setNewDownloadOpen}
        onCreated={(task) => {
          selectTask(task.id);
          void refreshTasks(task.id);
        }}
      />
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
    </div>
  );
}
