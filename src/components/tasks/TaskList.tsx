import { useMemo } from "react";

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
  const tasks = useTaskStore((s) => s.tasks);
  const nav = useTaskStore((s) => s.nav);
  const search = useTaskStore((s) => s.search);
  const selectedId = useTaskStore((s) => s.selectedId);
  const selectTask = useTaskStore((s) => s.selectTask);
  const loading = useTaskStore((s) => s.loading);
  const error = useTaskStore((s) => s.error);

  const filtered = useMemo(
    () => filterTasks(tasks, nav, search),
    [tasks, nav, search],
  );

  if (nav === "settings") {
    return <SettingsPlaceholder />;
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
          <p className="px-4 py-8 text-sm text-text-muted">Loading tasks…</p>
        ) : filtered.length === 0 ? (
          <p className="px-4 py-8 text-sm text-text-muted">No tasks in this view.</p>
        ) : (
          filtered.map((task) => (
            <TaskRow
              key={task.id}
              task={task}
              selected={task.id === selectedId}
              onSelect={() => selectTask(task.id)}
              onToggleTransfer={onToggleTransfer}
              onRetry={onRetry}
              onOpenFile={onOpenFile}
              onOpenFolder={onOpenFolder}
            />
          ))
        )}
      </ScrollArea>
    </div>
  );
}

function SettingsPlaceholder() {
  return (
    <div className="flex flex-1 flex-col gap-4 p-6 text-sm text-text-secondary">
      <h2 className="text-base font-medium text-text-primary">Settings</h2>
      <p>Default download directory, speed limits, and browser integration will ship in the next milestone.</p>
    </div>
  );
}
