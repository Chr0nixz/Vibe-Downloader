import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { SettingsPage } from "@/components/settings/SettingsPage";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { TaskRow } from "@/components/tasks/TaskRow";
import {
  failureKind,
  filterTasks,
  useTaskStore,
  type FileTypeFilter,
  type ResumeFilter,
  type TaskSortKey,
} from "@/stores/task-store";
import type { Task } from "@/types/task";
import type { RecoveryAction } from "@/generated/bindings";

export function TaskList({
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
  onResolveAttention,
  onBulkPause,
  onBulkResume,
  onBulkRetry,
  onBulkDelete,
  onBulkOpenFolder,
}: {
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onBulkPause: (tasks: Task[]) => void;
  onBulkResume: (tasks: Task[]) => void;
  onBulkRetry: (tasks: Task[]) => void;
  onBulkDelete: (tasks: Task[]) => void;
  onBulkOpenFolder: (tasks: Task[]) => void;
}) {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const nav = useTaskStore((s) => s.nav);
  const search = useTaskStore((s) => s.search);
  const selectedId = useTaskStore((s) => s.selectedId);
  const selectedIds = useTaskStore((s) => s.selectedIds);
  const sortKey = useTaskStore((s) => s.sortKey);
  const sortDirection = useTaskStore((s) => s.sortDirection);
  const filters = useTaskStore((s) => s.filters);
  const selectTask = useTaskStore((s) => s.selectTask);
  const setSelectedIds = useTaskStore((s) => s.setSelectedIds);
  const setTaskSelected = useTaskStore((s) => s.setTaskSelected);
  const clearSelectedIds = useTaskStore((s) => s.clearSelectedIds);
  const setSort = useTaskStore((s) => s.setSort);
  const setFilters = useTaskStore((s) => s.setFilters);
  const setDetailOpen = useTaskStore((s) => s.setDetailOpen);
  const loading = useTaskStore((s) => s.loading);
  const error = useTaskStore((s) => s.error);

  const filtered = useMemo(
    () => filterTasks(tasks, nav, search, sortKey, sortDirection, filters),
    [tasks, nav, search, sortKey, sortDirection, filters],
  );
  const selectedTasks = useMemo(
    () => tasks.filter((task) => selectedIds.includes(task.id)),
    [selectedIds, tasks],
  );
  const visibleSelectedCount = filtered.filter((task) =>
    selectedIds.includes(task.id),
  ).length;
  const allVisibleSelected =
    filtered.length > 0 && visibleSelectedCount === filtered.length;
  const sourceOptions = useMemo(
    () => Array.from(new Set(tasks.map((task) => task.sourceKey))).sort(),
    [tasks],
  );
  const failureOptions = useMemo(
    () =>
      Array.from(
        new Set(
          tasks
            .map(failureKind)
            .filter((kind) => kind !== "none"),
        ),
      ).sort(),
    [tasks],
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

      <div className="flex flex-col gap-2 border-b border-border-subtle bg-surface-base px-3 py-2 text-xs md:flex-row md:items-center md:justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex h-8 items-center gap-2 rounded-md border border-border-subtle px-2 text-text-secondary">
            <input
              type="checkbox"
              checked={allVisibleSelected}
              disabled={filtered.length === 0}
              onChange={(event) => {
                if (event.target.checked) {
                  setSelectedIds(filtered.map((task) => task.id));
                } else {
                  clearSelectedIds();
                }
              }}
              className="h-4 w-4 accent-accent-primary"
            />
            {t("taskList.selectVisible", { count: filtered.length })}
          </label>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={selectedTasks.length === 0}
            onClick={() => onBulkPause(selectedTasks)}
          >
            {t("taskList.bulkPause")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={selectedTasks.length === 0}
            onClick={() => onBulkResume(selectedTasks)}
          >
            {t("taskList.bulkResume")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={selectedTasks.length === 0}
            onClick={() => onBulkRetry(selectedTasks)}
          >
            {t("taskList.bulkRetry")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={selectedTasks.length === 0}
            onClick={() => onBulkOpenFolder(selectedTasks)}
          >
            {t("taskList.bulkOpenFolder")}
          </Button>
          <Button
            type="button"
            variant="danger"
            size="sm"
            disabled={selectedTasks.length === 0}
            onClick={() => onBulkDelete(selectedTasks)}
          >
            {t("taskList.bulkDelete", { count: selectedTasks.length })}
          </Button>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <SelectControl
            label={t("taskList.sort")}
            value={`${sortKey}:${sortDirection}`}
            onChange={(value) => {
              const [key, direction] = value.split(":") as [TaskSortKey, "asc" | "desc"];
              setSort(key, direction);
            }}
            options={[
              ["updated_at:desc", t("taskList.sortUpdatedDesc")],
              ["created_at:desc", t("taskList.sortCreatedDesc")],
              ["file_size:desc", t("taskList.sortSizeDesc")],
              ["progress:desc", t("taskList.sortProgressDesc")],
              ["speed:desc", t("taskList.sortSpeedDesc")],
              ["status:asc", t("taskList.sortStatusAsc")],
            ]}
          />
          <SelectControl
            label={t("taskList.fileType")}
            value={filters.fileType}
            onChange={(value) => setFilters({ fileType: value as FileTypeFilter })}
            options={[
              ["all", t("taskList.allFileTypes")],
              ["archive", t("taskList.fileTypeArchive")],
              ["image", t("taskList.fileTypeImage")],
              ["video", t("taskList.fileTypeVideo")],
              ["document", t("taskList.fileTypeDocument")],
              ["app", t("taskList.fileTypeApp")],
              ["other", t("taskList.fileTypeOther")],
            ]}
          />
          <SelectControl
            label={t("taskList.source")}
            value={filters.source}
            onChange={(value) => setFilters({ source: value })}
            options={[
              ["all", t("taskList.allSources")],
              ...sourceOptions.map((source) => [source, source] as const),
            ]}
          />
          <SelectControl
            label={t("taskList.failure")}
            value={filters.failure}
            onChange={(value) => setFilters({ failure: value })}
            options={[
              ["all", t("taskList.allFailures")],
              ...failureOptions.map((failure) => [
                failure,
                t(`taskList.failure_${failure}`, { defaultValue: failure }),
              ] as const),
            ]}
          />
          <SelectControl
            label={t("taskList.resume")}
            value={filters.resume}
            onChange={(value) => setFilters({ resume: value as ResumeFilter })}
            options={[
              ["all", t("taskList.allResume")],
              ["resumable", t("taskList.resumable")],
              ["single_connection", t("taskList.singleConnection")],
            ]}
          />
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        {loading ? (
          <p className="px-4 py-8 text-sm text-text-muted">{t("taskList.loading")}</p>
        ) : filtered.length === 0 ? (
          <p className="px-4 py-8 text-sm text-text-muted">{t("taskList.empty")}</p>
        ) : (
          <div
            role="listbox"
            aria-label={t("taskList.aria")}
            aria-activedescendant={selectedId ? `task-option-${selectedId}` : undefined}
            onKeyDown={handleListboxKeyDown}
            className="space-y-2.5 p-2.5 sm:p-3 md:p-4"
          >
            {filtered.map((task, index) => (
              <TaskRow
                key={task.id}
                task={task}
                selected={task.id === selectedId}
                multiSelected={selectedIds.includes(task.id)}
                position={index + 1}
                setSize={filtered.length}
                onSelectTask={selectAndFocus}
                onToggleSelected={setTaskSelected}
                onNavigate={navigateRow}
                onToggleTransfer={onToggleTransfer}
                onRetry={onRetry}
                onOpenFile={onOpenFile}
                onOpenFolder={onOpenFolder}
                onResolveAttention={onResolveAttention}
              />
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

function SelectControl({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly (readonly [string, string])[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="flex h-8 items-center gap-1 text-text-muted">
      <span className="sr-only">{label}</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-8 rounded-md border border-border-subtle bg-surface-root px-2 text-xs text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
        title={label}
      >
        {options.map(([optionValue, optionLabel]) => (
          <option key={optionValue} value={optionValue}>
            {optionLabel}
          </option>
        ))}
      </select>
    </label>
  );
}
