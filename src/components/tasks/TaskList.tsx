import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { SettingsPage } from "@/components/settings/SettingsPage";
import { ScrollArea } from "@/components/ui/scroll-area";
import { TaskRow } from "@/components/tasks/TaskRow";
import { filterTasks, useTaskStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

export function TaskList({
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
}: {
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
}) {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const nav = useTaskStore((s) => s.nav);
  const search = useTaskStore((s) => s.search);
  const selectedId = useTaskStore((s) => s.selectedId);
  const expandedTaskIds = useTaskStore((s) => s.expandedTaskIds);
  const speedHistoryByTaskId = useTaskStore((s) => s.speedHistoryByTaskId);
  const selectTask = useTaskStore((s) => s.selectTask);
  const setDetailOpen = useTaskStore((s) => s.setDetailOpen);
  const toggleTaskExpanded = useTaskStore((s) => s.toggleTaskExpanded);
  const loading = useTaskStore((s) => s.loading);
  const error = useTaskStore((s) => s.error);

  const filtered = useMemo(
    () => filterTasks(tasks, nav, search),
    [tasks, nav, search],
  );

  const selectAndFocus = useCallback(
    (taskId: string) => {
      selectTask(taskId);
      setDetailOpen(true);
      requestAnimationFrame(() => {
        document.getElementById(`task-option-${taskId}`)?.focus();
      });
    },
    [selectTask, setDetailOpen],
  );

  const navigateRow = useCallback(
    (direction: "next" | "prev") => {
      if (filtered.length === 0) return;
      const currentIndex = filtered.findIndex((task) => task.id === selectedId);
      const startIndex = currentIndex >= 0 ? currentIndex : 0;
      const nextIndex =
        direction === "next"
          ? Math.min(filtered.length - 1, startIndex + 1)
          : Math.max(0, startIndex - 1);
      const nextTask = filtered[nextIndex];
      if (nextTask) selectAndFocus(nextTask.id);
    },
    [filtered, selectAndFocus, selectedId],
  );

  const handleListboxKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (filtered.length === 0) return;

      if (event.key === "Home") {
        event.preventDefault();
        selectAndFocus(filtered[0].id);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        selectAndFocus(filtered[filtered.length - 1].id);
      }
    },
    [filtered, selectAndFocus],
  );

  if (nav === "settings") {
    return <SettingsPage />;
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      {error ? (
        <div
          className="border-b border-status-danger/30 bg-status-danger/10 px-4 py-2 text-sm text-status-danger"
          role="alert"
        >
          {error}
        </div>
      ) : null}

      <ScrollArea className="min-h-0 flex-1">
        {loading ? (
          <p className="px-4 py-8 text-sm text-text-muted">{t("taskList.loading")}</p>
        ) : filtered.length === 0 ? (
          <p className="px-4 py-8 text-sm text-text-muted">{t("taskList.empty")}</p>
        ) : (
          <div
            role="listbox"
            aria-label={t("taskList.aria")}
            aria-activedescendant={
              selectedId ? `task-option-${selectedId}` : undefined
            }
            onKeyDown={handleListboxKeyDown}
            className="space-y-2.5 p-3 md:p-4"
          >
            {filtered.map((task) => (
              <TaskRow
                key={task.id}
                task={task}
                selected={task.id === selectedId}
                expanded={expandedTaskIds.includes(task.id)}
                speedHistory={speedHistoryByTaskId[task.id] ?? []}
                onSelect={() => selectAndFocus(task.id)}
                onNavigate={navigateRow}
                onToggleExpanded={() => toggleTaskExpanded(task.id)}
                onToggleTransfer={onToggleTransfer}
                onRetry={onRetry}
                onOpenFile={onOpenFile}
                onOpenFolder={onOpenFolder}
              />
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}
