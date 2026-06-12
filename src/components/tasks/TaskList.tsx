import { useVirtualizer } from "@tanstack/react-virtual";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useReducedMotion } from "framer-motion";
import { ChevronDown, Plus, Search, SlidersHorizontal } from "lucide-react";

const SettingsPage = lazy(() =>
  import("@/components/settings/SettingsPage").then((m) => ({
    default: m.SettingsPage,
  })),
);
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { TaskRow } from "@/components/tasks/TaskRow";
import {
  failureKind,
  taskCursorInput,
  useTaskStore,
  type FileTypeFilter,
  type ResumeFilter,
  type TaskSortKey,
} from "@/stores/task-store";
import { readShellLayout } from "@/hooks/use-shell-layout";
import { listTasksCursor } from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
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
  const reduceMotion = !!useReducedMotion();
  const [toolPanelOpen, setToolPanelOpen] = useState(false);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const loadingPageRef = useRef(false);
  const tasks = useTaskStore((s) => s.tasks);
  const total = useTaskStore((s) => s.total);
  const nextCursor = useTaskStore((s) => s.nextCursor);
  const hasMore = useTaskStore((s) => s.hasMore);
  const filterOptions = useTaskStore((s) => s.filterOptions);
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
  const setTaskCursorPage = useTaskStore((s) => s.setTaskCursorPage);
  const loading = useTaskStore((s) => s.loading);
  const setLoading = useTaskStore((s) => s.setLoading);
  const error = useTaskStore((s) => s.error);
  const setError = useTaskStore((s) => s.setError);

  const activeFilterCount = useMemo(() => {
    let n = 0;
    if (filters.fileType !== "all") n++;
    if (filters.source !== "all") n++;
    if (filters.failure !== "all") n++;
    if (filters.resume !== "all") n++;
    return n;
  }, [filters]);

  const filtered = tasks;
  const filteredRef = useRef(filtered);
  filteredRef.current = filtered;
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  const loadPage = useCallback(async (cursor: string | null, append = false) => {
    if (loadingPageRef.current) return;
    loadingPageRef.current = true;
    if (!append) setLoading(true);
    try {
      const result = await listTasksCursor(taskCursorInput(cursor));
      setTaskCursorPage(
        result.items,
        result.totalEstimate,
        result.nextCursor,
        result.filterOptions,
        append,
      );
      if (!append) {
        const currentSelectedId = useTaskStore.getState().selectedId;
        if (
          result.items.length > 0 &&
          (!currentSelectedId || !result.items.some((task) => task.id === currentSelectedId))
        ) {
          selectTask(result.items[0].id);
          if (readShellLayout() === "wide") {
            setDetailOpen(true);
          }
        } else if (result.items.length === 0) {
          selectTask(null);
        }
      }
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
      loadingPageRef.current = false;
    }
  }, [selectTask, setDetailOpen, setError, setLoading, setTaskCursorPage]);

  useEffect(() => {
    void loadPage(null, false);
  }, [filters, loadPage, nav, search, sortDirection, sortKey]);

  /* Keep latest infinite-scroll state in refs so the virtualizer's onChange
     callback always sees fresh values without needing them as deps. */
  const hasMoreRef = useRef(hasMore);
  hasMoreRef.current = hasMore;
  const nextCursorRef = useRef(nextCursor);
  nextCursorRef.current = nextCursor;

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => 142, // ~132px row + 10px gap
    overscan: 6,
    getItemKey: (index) => filtered[index]?.id ?? index,
    onChange: (instance) => {
      const items = instance.getVirtualItems();
      if (items.length === 0) return;
      const lastItem = items[items.length - 1];
      const scrollLen = instance.scrollRect?.height ?? 0;
      const totalSize = instance.getTotalSize();
      if (
        hasMoreRef.current &&
        !loadingPageRef.current &&
        totalSize - (lastItem.start + lastItem.size) < 700 &&
        scrollLen > 0
      ) {
        void loadPage(nextCursorRef.current, true);
      }
    },
  });

  // Scroll to top when filter / sort / search changes.
  useEffect(() => {
    virtualizer.scrollToOffset(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filters, nav, search, sortDirection, sortKey]);
  const selectedTasks = useMemo(
    () => tasks.filter((task) => selectedIds.includes(task.id)),
    [selectedIds, tasks],
  );
  const visibleSelectedCount = useMemo(
    () => filtered.filter((task) => selectedIds.includes(task.id)).length,
    [filtered, selectedIds],
  );
  const allVisibleSelected =
    filtered.length > 0 && visibleSelectedCount === filtered.length;
  const sourceOptions = filterOptions.sources;
  const failureOptions = useMemo(
    () =>
      filterOptions.failureCategories.length > 0
        ? filterOptions.failureCategories
        : Array.from(
            new Set(
              tasks
                .map(failureKind)
                .filter((kind) => kind !== "none"),
            ),
          ).sort(),
    [filterOptions.failureCategories, tasks],
  );

  const selectAndFocus = useCallback(
    (taskId: string) => {
      selectTask(taskId);
      setDetailOpen(true);
      const list = filteredRef.current;
      const index = list.findIndex((task) => task.id === taskId);
      if (index >= 0) {
        virtualizer.scrollToIndex(index, { align: "center" });
      }
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          document.getElementById(`task-option-${taskId}`)?.focus();
        });
      });
    },
    [selectTask, setDetailOpen, virtualizer],
  );

  // Ensure the selected row is scrolled into view (e.g. after initial load).
  useEffect(() => {
    if (!selectedId || filtered.length === 0) return;
    const index = filtered.findIndex((task) => task.id === selectedId);
    if (index >= 0) {
      virtualizer.scrollToIndex(index, { align: "center" });
    }
  }, [selectedId, filtered, virtualizer]);

  const navigateRow = useCallback(
    (direction: "next" | "prev") => {
      const list = filteredRef.current;
      if (list.length === 0) return;
      const currentId = selectedIdRef.current;
      const currentIndex = list.findIndex((task) => task.id === currentId);
      const startIndex = currentIndex >= 0 ? currentIndex : 0;
      const nextIndex =
        direction === "next"
          ? Math.min(list.length - 1, startIndex + 1)
          : Math.max(0, startIndex - 1);
      const nextTask = list[nextIndex];
      if (nextTask) selectAndFocus(nextTask.id);
    },
    [selectAndFocus],
  );

  const handleListboxKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const list = filteredRef.current;
      if (list.length === 0) return;

      if (event.key === "Home") {
        event.preventDefault();
        virtualizer.scrollToIndex(0, { align: "start" });
        selectAndFocus(list[0].id);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        virtualizer.scrollToIndex(list.length - 1, { align: "end" });
        selectAndFocus(list[list.length - 1].id);
      }
    },
    [selectAndFocus, virtualizer],
  );

  if (nav === "settings") {
    return (
      <Suspense fallback={<div className="flex min-h-0 min-w-0 flex-1 animate-pulse bg-surface-root" />}>
        <SettingsPage />
      </Suspense>
    );
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      {error ? (
        <div
          className="border-b border-border-danger bg-status-danger/10 px-4 py-2 text-sm text-status-danger"
          role="alert"
        >
          {error}
        </div>
      ) : null}

      <div className="border-b border-border-subtle bg-surface-base/70 px-3 py-2 text-xs">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-expanded={toolPanelOpen}
          aria-controls="task-list-tool-panel"
          aria-label={
            !toolPanelOpen && activeFilterCount > 0
              ? t("taskList.toolPanelActive", { count: activeFilterCount })
              : t(toolPanelOpen ? "taskList.hideToolPanel" : "taskList.showToolPanel")
          }
          onClick={() => setToolPanelOpen((open) => !open)}
          className="text-text-muted"
        >
          <SlidersHorizontal className="h-4 w-4" aria-hidden="true" />
          {!toolPanelOpen && activeFilterCount > 0 ? (
            <span
              className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent-primary px-1 text-[10px] font-bold leading-none text-white"
              aria-hidden="true"
            >
              {activeFilterCount}
            </span>
          ) : null}
          {t("taskList.toolPanel")}
          <ChevronDown
            className={`h-4 w-4 transition-transform duration-ui ${
              toolPanelOpen ? "rotate-180" : ""
            }`}
            aria-hidden="true"
          />
        </Button>
        {toolPanelOpen ? (
          <div
            id="task-list-tool-panel"
            className="mt-2 flex flex-col gap-2 md:flex-row md:items-center md:justify-between"
          >
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex h-11 items-center gap-2 rounded-md border border-border-subtle px-2 text-text-secondary md:h-8">
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
                  className="h-5 w-5 accent-accent-primary md:h-4 md:w-4"
                />
                {t("taskList.selectVisible", { count: filtered.length })}
              </label>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8"
                disabled={selectedTasks.length === 0}
                onClick={() => onBulkPause(selectedTasks)}
              >
                {t("taskList.bulkPause")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8"
                disabled={selectedTasks.length === 0}
                onClick={() => onBulkResume(selectedTasks)}
              >
                {t("taskList.bulkResume")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8"
                disabled={selectedTasks.length === 0}
                onClick={() => onBulkRetry(selectedTasks)}
              >
                {t("taskList.bulkRetry")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8"
                disabled={selectedTasks.length === 0}
                onClick={() => onBulkOpenFolder(selectedTasks)}
              >
                {t("taskList.bulkOpenFolder")}
              </Button>
              <Button
                type="button"
                variant="danger"
                size="sm"
                className="h-11 md:h-8"
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
        ) : null}
      </div>

      <div
        ref={scrollContainerRef}
        className="min-h-0 flex-1 overflow-y-auto"
      >
        {loading ? (
          <TaskListLoadingSkeleton label={t("taskList.loading")} />
        ) : filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-4 px-6 py-20 text-center">
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent-primary/8">
              {search ? (
                <Search className="h-7 w-7 text-text-muted" />
              ) : (
                <Plus className="h-7 w-7 text-accent-primary/70" />
              )}
            </div>
            <div className="space-y-1.5">
              <p className="text-sm font-medium text-text-primary">
                {search ? t("taskList.emptySearch") : t("taskList.empty")}
              </p>
              {!search ? (
                <p className="max-w-xs text-xs leading-relaxed text-text-muted">
                  {t("taskList.emptyHint")}
                </p>
              ) : null}
            </div>
          </div>
        ) : (
          <>
            <div
              role="listbox"
              aria-label={t("taskList.aria")}
              onKeyDown={handleListboxKeyDown}
              className="relative [--lp:10px] sm:[--lp:12px] md:[--lp:16px] p-2.5 sm:p-3 md:p-4 pt-[var(--lp)]! pb-[var(--lp)]!"
              style={{ height: `calc(${virtualizer.getTotalSize()}px + var(--lp, 16px) * 2)` }}
            >
              {virtualizer.getVirtualItems().map((virtualRow) => {
                const task = filtered[virtualRow.index];
                return (
                  <div
                    key={virtualRow.key}
                    data-index={virtualRow.index}
                    ref={virtualizer.measureElement}
                    className="absolute inset-x-2.5 sm:inset-x-3 md:inset-x-4"
                    style={{
                      top: 0,
                      transform: `translateY(calc(${virtualRow.start}px + var(--lp, 16px)))`,
                      paddingBottom: virtualRow.index < filtered.length - 1 ? 10 : 0,
                    }}
                  >
                    <TaskRow
                      task={task}
                      selected={task.id === selectedId}
                      multiSelected={selectedIds.includes(task.id)}
                      isFirstFocusable={!selectedId && virtualRow.index === 0}
                      reduceMotion={reduceMotion}
                      position={virtualRow.index + 1}
                      setSize={total}
                      onSelectTask={selectAndFocus}
                      onToggleSelected={setTaskSelected}
                      onNavigate={navigateRow}
                      onToggleTransfer={onToggleTransfer}
                      onRetry={onRetry}
                      onOpenFile={onOpenFile}
                      onOpenFolder={onOpenFolder}
                      onResolveAttention={onResolveAttention}
                    />
                  </div>
                );
              })}
            </div>
            {hasMore ? (
              <p className="px-2 py-3 text-center text-xs text-text-muted">
                {t("taskList.loadingMore", { count: Math.max(0, total - filtered.length) })}
              </p>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

function TaskListLoadingSkeleton({ label }: { label: string }) {
  return (
    <div className="p-2.5 sm:p-3 md:p-4" role="status" aria-live="polite" aria-label={label}>
      <span className="sr-only">{label}</span>
      <div className="space-y-2.5">
        {Array.from({ length: 5 }).map((_, index) => (
          <div
            key={index}
            className="overflow-hidden rounded-lg border border-border-subtle/60 bg-surface-base/60 px-3 py-3.5 sm:px-3.5 md:px-4"
          >
            <div className="animate-pulse">
              <div className="flex min-w-0 gap-3">
                <div className="mt-0.5 h-8 w-8 shrink-0 rounded bg-surface-raised" />
                <div className="min-w-0 flex-1 space-y-3">
                  <div className="flex items-center gap-2">
                    <div className="h-4 w-44 max-w-[52%] rounded bg-surface-raised" />
                    <div className="h-4 w-20 rounded-full bg-surface-raised/80" />
                  </div>
                  <div className="h-3 w-32 rounded bg-surface-raised/70" />
                  <div className="h-3 w-3/5 rounded bg-surface-raised/70" />
                  <div className="h-2.5 rounded-full bg-surface-raised">
                    <div className="h-full w-1/3 rounded-full bg-accent-primary/25" />
                  </div>
                </div>
                <div className="hidden min-w-36 flex-col items-end gap-2 md:flex">
                  <div className="h-5 w-20 rounded bg-surface-raised" />
                  <div className="h-3 w-28 rounded bg-surface-raised/70" />
                  <div className="h-3 w-24 rounded bg-surface-raised/70" />
                  <div className="mt-1 flex gap-1.5">
                    <div className="h-8 w-8 rounded bg-surface-raised" />
                    <div className="h-8 w-8 rounded bg-surface-raised" />
                    <div className="h-8 w-8 rounded bg-surface-raised" />
                  </div>
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
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
    <label className="flex h-11 items-center gap-1.5 text-text-muted md:h-8">
      <span className="text-[11px] font-medium text-text-muted/70">
        {label}
      </span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger
          aria-label={label}
          title={label}
          className="w-auto min-w-[6rem] px-2.5 text-xs font-medium"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map(([optionValue, optionLabel]) => (
            <SelectItem key={optionValue} value={optionValue}>
              {optionLabel}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  );
}
