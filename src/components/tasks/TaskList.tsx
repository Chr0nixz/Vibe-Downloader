import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown, MoreHorizontal, Plus, Search, SlidersHorizontal, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDebouncedValue } from "@/hooks/use-debounced-value";

const SettingsPage = lazy(() =>
  import("@/components/settings/SettingsPage").then((m) => ({
    default: m.SettingsPage,
  })),
);

import { ListContextMenu } from "@/components/tasks/TaskContextMenu";
import { TaskRow } from "@/components/tasks/TaskRow";
import { TASK_ROW_ESTIMATED_SIZE } from "@/components/tasks/task-layout";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { RecoveryAction } from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { listTasksCursor } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  type FileTypeFilter,
  failureKind,
  type ResumeFilter,
  taskCursorInput,
  useTaskDataStore,
  useTaskUIStore,
} from "@/stores/task-store";
import type { Task } from "@/types/task";

export function TaskList({
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onResolveAttention,
  onDelete,
  onNewDownload,
  onBulkPause,
  onBulkResume,
  onBulkRetry,
  onBulkDelete,
  onBulkOpenFolder,
  onBulkExport,
}: {
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onDelete: (task: Task) => void;
  onNewDownload: () => void;
  onBulkPause: (tasks: Task[]) => void;
  onBulkResume: (tasks: Task[]) => void;
  onBulkRetry: (tasks: Task[]) => void;
  onBulkDelete: (tasks: Task[]) => void;
  onBulkOpenFolder: (tasks: Task[]) => void;
  onBulkExport: (tasks: Task[], format: "json" | "csv") => void;
}) {
  const { t } = useTranslation();
  const reduceMotion = !!useReducedMotion();
  const [toolPanelOpen, setToolPanelOpen] = useState(false);
  const [statusAnnouncement, setStatusAnnouncement] = useState("");
  const prevTaskStatusesRef = useRef<Record<string, string>>({});
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const loadingPageRef = useRef(false);
  const initialLoadDoneRef = useRef(false);
  const taskIds = useTaskDataStore((s) => s.taskIds);
  const total = useTaskDataStore((s) => s.total);
  const nextCursor = useTaskDataStore((s) => s.nextCursor);
  const hasMore = useTaskDataStore((s) => s.hasMore);
  const filterOptions = useTaskDataStore((s) => s.filterOptions);
  const nav = useTaskUIStore((s) => s.nav);
  const search = useTaskUIStore((s) => s.search);
  // Debounce the value that drives backend queries and scroll resets so rapid
  // typing doesn't fire a request per keystroke. The raw `search` value above
  // is still used for the empty-state label so the UI stays responsive.
  const debouncedSearch = useDebouncedValue(search, 300);
  const selectedId = useTaskUIStore((s) => s.selectedId);
  const selectedIds = useTaskUIStore((s) => s.selectedIds);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const filters = useTaskUIStore((s) => s.filters);
  const selectTask = useTaskUIStore((s) => s.selectTask);
  const setSelectedIds = useTaskUIStore((s) => s.setSelectedIds);
  const setTaskSelected = useTaskUIStore((s) => s.setTaskSelected);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setFilters = useTaskUIStore((s) => s.setFilters);
  const setDetailOpen = useTaskUIStore((s) => s.setDetailOpen);
  const setTaskCursorPage = useTaskDataStore((s) => s.setTaskCursorPage);
  const loading = useTaskDataStore((s) => s.loading);
  const setLoading = useTaskDataStore((s) => s.setLoading);
  const error = useTaskDataStore((s) => s.error);
  const setError = useTaskDataStore((s) => s.setError);

  const activeFilterCount = useMemo(() => {
    let n = 0;
    if (filters.fileType !== "all") n++;
    if (filters.source !== "all") n++;
    if (filters.failure !== "all") n++;
    if (filters.resume !== "all") n++;
    return n;
  }, [filters]);

  const filtered = taskIds;
  const filteredRef = useRef(filtered);
  filteredRef.current = filtered;

  // Announce task status changes for screen readers
  useEffect(() => {
    return useTaskDataStore.subscribe((state) => {
      const prev = prevTaskStatusesRef.current;
      for (const taskId of state.taskIds) {
        const task = state.taskById[taskId];
        if (task && prev[task.id] && prev[task.id] !== task.status) {
          setStatusAnnouncement(
            t("taskList.statusChanged", {
              name: task.fileName,
              status: t(`task.status.${task.status}`),
            }),
          );
          break;
        }
      }
      const next: Record<string, string> = {};
      for (const taskId of state.taskIds) {
        const task = state.taskById[taskId];
        if (task) next[task.id] = task.status;
      }
      prevTaskStatusesRef.current = next;
    });
  }, [t]);
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  const loadPage = useCallback(
    async (cursor: string | null, append = false) => {
      if (loadingPageRef.current) return;
      loadingPageRef.current = true;
      if (!append) setLoading(true);
      try {
        const result = await listTasksCursor(taskCursorInput(cursor));
        setTaskCursorPage(result.items, result.totalEstimate, result.nextCursor, result.filterOptions, append);
        if (!append) {
          const currentSelectedId = useTaskUIStore.getState().selectedId;
          if (
            result.items.length > 0 &&
            (!currentSelectedId || !result.items.some((task) => task.id === currentSelectedId))
          ) {
            selectTask(result.items[0].id);
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
        if (!append) initialLoadDoneRef.current = true;
      }
    },
    [selectTask, setDetailOpen, setError, setLoading, setTaskCursorPage],
  );

  useEffect(() => {
    void loadPage(null, false);
  }, [filters, loadPage, nav, debouncedSearch, sortDirection, sortKey]);

  /* Keep latest infinite-scroll state in refs so the virtualizer's onChange
     callback always sees fresh values without needing them as deps. */
  const hasMoreRef = useRef(hasMore);
  hasMoreRef.current = hasMore;
  const nextCursorRef = useRef(nextCursor);
  nextCursorRef.current = nextCursor;

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => TASK_ROW_ESTIMATED_SIZE, // compact row + 8px gap; failed/expanded rows are measured.
    overscan: 6,
    getItemKey: (index) => filtered[index] ?? index,
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
  }, [filters, nav, debouncedSearch, sortDirection, sortKey]);
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const selectedTasks = useCallback(() => {
    const { taskById } = useTaskDataStore.getState();
    return selectedIds.map((id) => taskById[id]).filter((task): task is Task => Boolean(task));
  }, [selectedIds]);
  const visibleSelectedCount = useMemo(
    () => filtered.filter((taskId) => selectedIdSet.has(taskId)).length,
    [filtered, selectedIdSet],
  );
  const allVisibleSelected = filtered.length > 0 && visibleSelectedCount === filtered.length;
  const sourceOptions = filterOptions.sources;
  const failureOptions = useMemo(
    () =>
      filterOptions.failureCategories.length > 0
        ? filterOptions.failureCategories
        : Array.from(
            new Set(
              taskIds
                .map((taskId) => useTaskDataStore.getState().taskById[taskId])
                .filter((task): task is Task => Boolean(task))
                .map(failureKind)
                .filter((kind) => kind !== "none"),
            ),
          ).sort(),
    [filterOptions.failureCategories, taskIds],
  );

  const selectAndFocus = useCallback(
    (taskId: string) => {
      selectTask(taskId);
      setDetailOpen(true);
      const list = filteredRef.current;
      const index = list.indexOf(taskId);
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

  const handleShiftSelect = useCallback(
    (anchorId: string, currentId: string) => {
      const list = filteredRef.current;
      const anchorIdx = list.indexOf(anchorId);
      const currentIdx = list.indexOf(currentId);
      if (anchorIdx === -1 || currentIdx === -1) return;
      const [start, end] = anchorIdx < currentIdx ? [anchorIdx, currentIdx] : [currentIdx, anchorIdx];
      setSelectedIds(list.slice(start, end + 1));
    },
    [setSelectedIds],
  );

  // Ensure the selected row is scrolled into view (e.g. after initial load).
  useEffect(() => {
    if (!selectedId || filtered.length === 0) return;
    const index = filtered.indexOf(selectedId);
    if (index >= 0) {
      virtualizer.scrollToIndex(index, { align: "center" });
    }
  }, [selectedId, filtered, virtualizer]);

  const navigateRow = useCallback(
    (direction: "next" | "prev") => {
      const list = filteredRef.current;
      if (list.length === 0) return;
      const currentId = selectedIdRef.current;
      const currentIndex = list.findIndex((taskId) => taskId === currentId);
      const startIndex = currentIndex >= 0 ? currentIndex : 0;
      const nextIndex = direction === "next" ? Math.min(list.length - 1, startIndex + 1) : Math.max(0, startIndex - 1);
      const nextTask = list[nextIndex];
      if (nextTask) selectAndFocus(nextTask);
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
        selectAndFocus(list[0]);
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        virtualizer.scrollToIndex(list.length - 1, { align: "end" });
        selectAndFocus(list[list.length - 1]);
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

      {/* Screen reader status announcements */}
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {statusAnnouncement}
      </div>

      {/* Selection bar — contextual, appears when rows are multi-selected */}
      {selectedIds.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-border-accent-subtle bg-accent-primary/[0.04] px-3 py-1.5 text-xs md:gap-2">
          <span className="font-medium text-text-secondary">
            {t("taskList.selectedCount", { count: selectedIds.length })}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 text-xs"
            onClick={() => setSelectedIds(filtered)}
            disabled={allVisibleSelected}
          >
            {t("taskList.selectVisible", { count: filtered.length })}
          </Button>
          <Button type="button" variant="ghost" size="sm" className="h-7 text-xs" onClick={clearSelectedIds}>
            <X className="mr-1 h-3 w-3" aria-hidden />
            {t("taskList.clearSelection")}
          </Button>
          <div className="mx-1 h-4 w-px bg-border-subtle" aria-hidden />
          <Button type="button" variant="ghost" size="sm" className="h-7" onClick={() => onBulkPause(selectedTasks())}>
            {t("taskList.bulkPause")}
          </Button>
          <Button type="button" variant="ghost" size="sm" className="h-7" onClick={() => onBulkResume(selectedTasks())}>
            {t("taskList.bulkResume")}
          </Button>
          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7"
                aria-label={t("taskList.moreBulkActions")}
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
                {t("taskList.more")}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-52">
              <div className="space-y-0.5" role="menu">
                <BulkMenuItem label={t("taskList.bulkRetry")} onClick={() => onBulkRetry(selectedTasks())} />
                <BulkMenuItem label={t("taskList.bulkOpenFolder")} onClick={() => onBulkOpenFolder(selectedTasks())} />
                <BulkMenuItem
                  label={t("taskList.selectVisible", { count: filtered.length })}
                  onClick={() => setSelectedIds(filtered)}
                  disabled={allVisibleSelected}
                />
                <BulkMenuItem label={t("taskList.exportJson")} onClick={() => onBulkExport(selectedTasks(), "json")} />
                <BulkMenuItem label={t("taskList.exportCsv")} onClick={() => onBulkExport(selectedTasks(), "csv")} />
              </div>
            </PopoverContent>
          </Popover>
          <Button
            type="button"
            variant="danger"
            size="sm"
            className="h-7"
            onClick={() => onBulkDelete(selectedTasks())}
          >
            {t("taskList.bulkDelete", { count: selectedIds.length })}
          </Button>
        </div>
      ) : null}

      {/* Active filter chips */}
      {activeFilterCount > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-border-subtle px-3 py-1.5">
          <FilterChip
            active={filters.fileType !== "all"}
            label={t("taskList.fileType")}
            value={
              filters.fileType !== "all"
                ? t(`taskList.fileType${filters.fileType.charAt(0).toUpperCase() + filters.fileType.slice(1)}`)
                : ""
            }
            onClear={() => setFilters({ fileType: "all" })}
          />
          <FilterChip
            active={filters.source !== "all"}
            label={t("taskList.source")}
            value={filters.source !== "all" ? filters.source : ""}
            onClear={() => setFilters({ source: "all" })}
          />
          <FilterChip
            active={filters.failure !== "all"}
            label={t("taskList.failure")}
            value={
              filters.failure !== "all"
                ? t(`taskList.failure_${filters.failure}`, { defaultValue: filters.failure })
                : ""
            }
            onClear={() => setFilters({ failure: "all" })}
          />
          <FilterChip
            active={filters.resume !== "all"}
            label={t("taskList.resume")}
            value={
              filters.resume !== "all"
                ? t(`taskList.${filters.resume === "resumable" ? "resumable" : "singleConnection"}`)
                : ""
            }
            onClear={() => setFilters({ resume: "all" })}
          />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-6 px-1.5 text-[10px] text-text-muted"
            onClick={() => setFilters({ fileType: "all", source: "all", failure: "all", resume: "all" })}
          >
            {t("taskList.clearAllFilters")}
          </Button>
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
              className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent-primary px-1 text-[10px] font-bold leading-none text-text-on-accent"
              aria-hidden="true"
            >
              {activeFilterCount}
            </span>
          ) : null}
          {t("taskList.toolPanel")}
          <ChevronDown
            className={`h-4 w-4 transition-transform duration-ui ${toolPanelOpen ? "rotate-180" : ""}`}
            aria-hidden="true"
          />
        </Button>
        {toolPanelOpen ? (
          <div id="task-list-tool-panel" className="mt-2 flex flex-wrap items-center gap-2">
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
              options={[["all", t("taskList.allSources")], ...sourceOptions.map((source) => [source, source] as const)]}
            />
            <SelectControl
              label={t("taskList.failure")}
              value={filters.failure}
              onChange={(value) => setFilters({ failure: value })}
              options={[
                ["all", t("taskList.allFailures")],
                ...failureOptions.map(
                  (failure) => [failure, t(`taskList.failure_${failure}`, { defaultValue: failure })] as const,
                ),
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
        ) : null}
      </div>

      <ListContextMenu onNewDownload={onNewDownload}>
        <div ref={scrollContainerRef} className="min-h-0 flex-1 overflow-y-auto">
          {loading && !initialLoadDoneRef.current ? (
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
                  <p className="max-w-xs text-xs leading-relaxed text-text-muted">{t("taskList.emptyHint")}</p>
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
                  const taskId = filtered[virtualRow.index];
                  return (
                    <div
                      key={virtualRow.key}
                      data-index={virtualRow.index}
                      ref={virtualizer.measureElement}
                      className="absolute inset-x-2.5 sm:inset-x-3 md:inset-x-4"
                      style={{
                        top: 0,
                        transform: `translateY(calc(${virtualRow.start}px + var(--lp, 16px)))`,
                        paddingBottom: virtualRow.index < filtered.length - 1 ? 8 : 0,
                      }}
                    >
                      <TaskRow
                        taskId={taskId}
                        selected={taskId === selectedId}
                        multiSelected={selectedIdSet.has(taskId)}
                        isFirstFocusable={!selectedId && virtualRow.index === 0}
                        reduceMotion={reduceMotion}
                        position={virtualRow.index + 1}
                        setSize={filtered.length}
                        onSelectTask={selectAndFocus}
                        onToggleSelected={setTaskSelected}
                        onNavigate={navigateRow}
                        onShiftSelect={handleShiftSelect}
                        onToggleTransfer={onToggleTransfer}
                        onRetry={onRetry}
                        onFinishLiveRecording={onFinishLiveRecording}
                        onOpenFile={onOpenFile}
                        onOpenFolder={onOpenFolder}
                        onDelete={onDelete}
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
      </ListContextMenu>
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
      <span className="text-[11px] font-medium text-text-muted">{label}</span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger aria-label={label} title={label} className="w-auto min-w-[6rem] px-2.5 text-xs font-medium">
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

function FilterChip({
  active,
  label,
  value,
  onClear,
}: {
  active: boolean;
  label: string;
  value: string;
  onClear: () => void;
}) {
  if (!active) return null;
  return (
    <span className="inline-flex items-center gap-1 rounded-full border border-border-accent-subtle bg-accent-primary/[0.04] px-2 py-0.5 text-[11px] font-medium text-text-secondary">
      <span className="text-text-muted">{label}:</span>
      {value}
      <button
        type="button"
        className="ml-0.5 -mr-0.5 inline-flex min-h-8 min-w-8 items-center justify-center rounded-full text-text-muted transition-colors hover:text-text-primary focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:outline-none"
        aria-label={`${label}: ${value}`}
        onClick={(event) => {
          event.stopPropagation();
          onClear();
        }}
      >
        <X className="h-3 w-3" aria-hidden />
      </button>
    </span>
  );
}

function BulkMenuItem({ label, onClick, disabled }: { label: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex h-8 w-full items-center rounded-md px-2 text-left text-sm text-text-secondary",
        "transition-[background-color,color] duration-[var(--motion-ui)] ease-out",
        "hover:bg-surface-raised hover:text-text-primary",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        "disabled:pointer-events-none disabled:opacity-40",
      )}
    >
      {label}
    </button>
  );
}
