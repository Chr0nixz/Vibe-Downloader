import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown, MoreHorizontal, Pause, Play, Plus, RotateCcw, Search, SlidersHorizontal, X } from "lucide-react";
import { useReducedMotion } from "motion/react";
import { lazy, memo, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDebouncedValue } from "@/hooks/use-debounced-value";

const SettingsPage = lazy(() =>
  import("@/components/settings/SettingsPage").then((m) => ({
    default: m.SettingsPage,
  })),
);

const AboutPage = lazy(() =>
  import("@/components/about/AboutPage").then((m) => ({
    default: m.AboutPage,
  })),
);

const AttentionCenter = lazy(() =>
  import("@/components/workspaces/AttentionCenter").then((m) => ({
    default: m.AttentionCenter,
  })),
);

const QueueCenter = lazy(() =>
  import("@/components/workspaces/QueueCenter").then((m) => ({
    default: m.QueueCenter,
  })),
);

import { ListContextMenu, type ReorderAction } from "@/components/tasks/TaskContextMenu";
import { TaskRow } from "@/components/tasks/TaskRow";
import { TASK_ROW_ESTIMATED_SIZE } from "@/components/tasks/task-layout";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { RecoveryAction, TaskPriority } from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { beginListLoad, createListLoadFlight, endListLoad, isCurrentListQueryEpoch } from "@/lib/list-query-epoch";
import { listTasksCursor } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  type FileTypeFilter,
  type ResumeFilter,
  taskCursorInput,
  useTaskDataStore,
  useTaskUIStore,
} from "@/stores/task-store";
import type { Task } from "@/types/task";

export const TaskList = memo(function TaskList({
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onResolveAttention,
  onReorder,
  onDelete,
  onDeleteFiles,
  onNewDownload,
  onBulkPause,
  onBulkResume,
  onBulkRetry,
  onBulkDelete,
  onBulkDeleteFiles,
  onBulkOpenFolder,
  onBulkExport,
  onOpenOnboarding,
  onCopyUrl,
  onCopyLocalPath,
  onShowDetails,
  onPasteAndCreate,
  onRefresh,
  onUpdateQueueOptions,
}: {
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onReorder?: (task: Task, action: ReorderAction) => void;
  onDelete: (task: Task) => void;
  onDeleteFiles?: (task: Task) => void;
  onNewDownload: () => void;
  onBulkPause: (tasks: Task[]) => void;
  onBulkResume: (tasks: Task[]) => void;
  onBulkRetry: (tasks: Task[]) => void;
  onBulkDelete: (tasks: Task[]) => void;
  onBulkDeleteFiles?: (tasks: Task[]) => void;
  onBulkOpenFolder: (tasks: Task[]) => void;
  onBulkExport: (tasks: Task[], format: "json" | "csv") => void;
  onOpenOnboarding: () => void;
  onCopyUrl?: (task: Task) => void;
  onCopyLocalPath?: (task: Task) => void;
  onShowDetails?: (task: Task) => void;
  onPasteAndCreate?: () => void;
  onRefresh?: () => void;
  onUpdateQueueOptions: (task: Task, patch: { priority?: TaskPriority; obeySchedule?: boolean }) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const reduceMotion = !!useReducedMotion();
  const [toolPanelOpen, setToolPanelOpen] = useState(false);
  const [statusAnnouncement, setStatusAnnouncement] = useState("");
  const prevTaskStatusesRef = useRef<Record<string, string>>({});
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const loadFlightRef = useRef(createListLoadFlight());
  const initialLoadDoneRef = useRef(false);
  const taskIds = useTaskDataStore((s) => s.taskIds);
  const storeFailureOptions = useTaskDataStore((s) => s.failureOptions);
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
  const selectionAnchorId = useTaskUIStore((s) => s.selectionAnchorId);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const filters = useTaskUIStore((s) => s.filters);
  const pendingDeleteIds = useTaskUIStore((s) => s.pendingDeleteIds);
  const selectTask = useTaskUIStore((s) => s.selectTask);
  const setSelectedIds = useTaskUIStore((s) => s.setSelectedIds);
  const setTaskSelected = useTaskUIStore((s) => s.setTaskSelected);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setFilters = useTaskUIStore((s) => s.setFilters);
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

  // Hide tasks that are in the soft-delete undo window so the list reflects
  // the deletion immediately while the undo toast is reachable.
  const pendingDeleteSet = useMemo(() => new Set(pendingDeleteIds), [pendingDeleteIds]);
  const filtered = useMemo(
    () => (pendingDeleteSet.size === 0 ? taskIds : taskIds.filter((id) => !pendingDeleteSet.has(id))),
    [pendingDeleteSet, taskIds],
  );
  const filteredRef = useRef(filtered);
  filteredRef.current = filtered;

  // Announce task status changes for screen readers.
  // Optimization: Zustand's bare `subscribe(listener)` fires on every state
  // change, including the ~4 Hz `patchTasksBatch` progress ticks that don't
  // touch `task.status`. Do a single O(n) diff pass that breaks early when
  // a status change is found; skip the per-tick Record allocation entirely
  // when nothing changed.
  useEffect(() => {
    return useTaskDataStore.subscribe((state) => {
      const prev = prevTaskStatusesRef.current;
      let changedTask: { name: string; status: string } | null = null;
      for (const taskId of state.taskIds) {
        const task = state.taskById[taskId];
        if (!task) continue;
        if (prev[task.id] && prev[task.id] !== task.status) {
          changedTask = { name: task.fileName, status: task.status };
          break;
        }
      }
      if (!changedTask) return;
      setStatusAnnouncement(
        t("taskList.statusChanged", {
          name: changedTask.name,
          status: t(`task.status.${changedTask.status}`),
        }),
      );
      // Rebuild the snapshot only after we know something changed.
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
      const begin = beginListLoad(loadFlightRef.current, append);
      if (begin.kind === "skip") return;
      const { epoch, role } = begin;
      if (role === "replace") setLoading(true);
      try {
        const result = await listTasksCursor(taskCursorInput(cursor, { search: debouncedSearch }));
        // ARC-07: ignore stale responses after a newer replace bumped the epoch.
        if (!isCurrentListQueryEpoch(epoch)) return;
        setTaskCursorPage(result.items, result.minimumTotal, result.nextCursor, result.filterOptions, append);
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
        if (isCurrentListQueryEpoch(epoch)) {
          setError(errorMessage(err));
        }
      } finally {
        if (role === "replace") {
          setLoading(false);
          initialLoadDoneRef.current = true;
        }
        if (endListLoad(loadFlightRef.current, role)) {
          void loadPage(null, false);
        }
      }
    },
    [debouncedSearch, filters, nav, selectTask, setError, setLoading, setTaskCursorPage, sortDirection, sortKey],
  );

  useEffect(() => {
    void loadPage(null, false);
  }, [loadPage]);

  const viewReloadToken = useTaskDataStore((s) => s.viewReloadToken);
  useEffect(() => {
    // ARC-08: membership/sort invalidation requests a replace reload through ARC-07.
    if (viewReloadToken === 0) return;
    void loadPage(null, false);
  }, [viewReloadToken, loadPage]);

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
        !loadFlightRef.current.replaceInFlight &&
        !loadFlightRef.current.appendInFlight &&
        totalSize - (lastItem.start + lastItem.size) < 700 &&
        scrollLen > 0
      ) {
        void loadPage(nextCursorRef.current, true);
      }
    },
  });

  // Scroll to top when filter / sort / search changes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: query fields intentionally trigger this imperative virtualizer reset.
  useEffect(() => {
    virtualizer.scrollToOffset(0);
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

  // P0c: Infer a single primary bulk action from the selection's status mix so the
  // selection bar can surface one prominent button instead of always showing both
  // Pause All and Resume All side-by-side (which differed only by icon and invited
  // mis-clicks on a 50-task selection). Returns null when statuses are mixed or
  // include terminal states (completed) where pause/resume/retry don't apply.
  //
  // Selector returns a primitive so Zustand's Object.is equality prevents re-renders
  // when progress patches (250ms) change taskById but not the status mix.
  const primaryBulkAction = useTaskDataStore<"pause" | "resume" | "retry" | null>((s) => {
    if (selectedIds.length === 0) return null;
    // P2: queued is grouped with downloading/retrying (pauseable), NOT with
    // paused/waiting_network (resumable). bulkResume filters out queued, so
    // grouping queued with paused produced a silent no-op "Resume" button for
    // all-queued selections. toggleTransfer and bulkPause both treat queued
    // as pauseable, so "Pause" is the correct inferred action for all-queued.
    let allActive = true; // downloading | retrying | queued
    let allPaused = true; // paused | waiting_network
    let allFailed = true; // failed | needs_attention
    for (const id of selectedIds) {
      const status = s.taskById[id]?.status;
      if (!status) return null;
      if (status !== "downloading" && status !== "retrying" && status !== "queued") allActive = false;
      if (status !== "paused" && status !== "waiting_network") allPaused = false;
      if (status !== "failed" && status !== "needs_attention") allFailed = false;
      if (status === "completed") return null;
    }
    if (allActive) return "pause";
    if (allPaused) return "resume";
    if (allFailed) return "retry";
    return null;
  });
  const sourceOptions = filterOptions.sources;
  // E-3: failureOptions is read from the store to avoid depending on taskById (which rebuilds its reference every 250ms) and causing per-frame recompute.
  // Prefer backend-provided failureCategories when available; otherwise use the store-computed value.
  const failureOptions =
    filterOptions.failureCategories.length > 0 ? filterOptions.failureCategories : storeFailureOptions;

  const selectAndFocus = useCallback(
    (taskId: string) => {
      selectTask(taskId);
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
    [selectTask, virtualizer],
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

  const handleListKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const list = filteredRef.current;
      if (list.length === 0) return;

      if (event.key === "ArrowDown") {
        event.preventDefault();
        navigateRow("next");
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        navigateRow("prev");
        return;
      }
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
    [navigateRow, selectAndFocus, virtualizer],
  );

  if (nav === "settings") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("settings.loading")} />}>
        <SettingsPage />
      </Suspense>
    );
  }

  if (nav === "about") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("about.loading")} />}>
        <AboutPage onOpenOnboarding={onOpenOnboarding} />
      </Suspense>
    );
  }

  if (nav === "attention") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("attentionCenter.loading")} />}>
        <AttentionCenter
          taskIds={filtered}
          loading={loading}
          error={error}
          hasMore={hasMore}
          onLoadMore={() => void loadPage(nextCursor, true)}
          onRetryLoad={() => void loadPage(null, false)}
          onResolve={onResolveAttention}
          onShowDetails={onShowDetails}
        />
      </Suspense>
    );
  }

  if (nav === "queue") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("queueCenter.loading")} />}>
        <QueueCenter
          taskIds={filtered}
          loading={loading}
          error={error}
          hasMore={hasMore}
          onLoadMore={() => void loadPage(nextCursor, true)}
          onRetryLoad={() => void loadPage(null, false)}
          onPause={onToggleTransfer}
          onReorder={onReorder}
          onShowDetails={onShowDetails}
          onUpdateOptions={onUpdateQueueOptions}
        />
      </Suspense>
    );
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      {error ? (
        <div
          className="flex flex-wrap items-center gap-2 border-b border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger md:px-4"
          role="alert"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-8 shrink-0 border-border-danger text-status-danger hover:bg-status-danger/10 hover:text-status-danger"
            onClick={() => void loadPage(null, false)}
            disabled={loading}
          >
            <RotateCcw className={cn("h-3.5 w-3.5", loading && "animate-spin")} aria-hidden />
            {t("taskList.retryLoad")}
          </Button>
        </div>
      ) : null}

      {/* Screen reader status announcements */}
      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {statusAnnouncement}
      </div>

      {/* Selection bar — contextual, appears when rows are multi-selected.
          Inferred primary + More menu for selection-scoped bulk actions.
          Global Pause all / Resume all live in the command palette only. */}
      {selectedIds.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-border-accent-subtle bg-accent-primary/[0.04] px-3 py-1.5 text-xs md:gap-2">
          <span className="font-medium text-text-secondary">
            {t("taskList.selectedCount", { count: selectedIds.length })}
          </span>
          <Button type="button" variant="ghost" size="sm" className="h-9 text-xs md:h-8" onClick={clearSelectedIds}>
            <X className="mr-1 h-3 w-3" aria-hidden />
            {t("taskList.clearSelection")}
          </Button>
          <div className="mx-1 h-4 w-px bg-border-subtle" aria-hidden />
          {/* Inferred primary action: Pause / Resume / Retry based on selection status mix. */}
          {primaryBulkAction === "pause" ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-11 md:h-8"
              onClick={() => onBulkPause(selectedTasks())}
            >
              <Pause className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {t("taskList.bulkPause")}
            </Button>
          ) : null}
          {primaryBulkAction === "resume" ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-11 md:h-8"
              onClick={() => onBulkResume(selectedTasks())}
            >
              <Play className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {t("taskList.bulkResume")}
            </Button>
          ) : null}
          {primaryBulkAction === "retry" ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-11 md:h-8"
              onClick={() => onBulkRetry(selectedTasks())}
            >
              <RotateCcw className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {t("taskList.bulkRetry")}
            </Button>
          ) : null}
          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8"
                aria-label={t("taskList.moreBulkActions")}
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
                {t("taskList.more")}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-52">
              <div className="space-y-0.5" role="menu">
                <BulkMenuItem label={t("taskList.bulkPause")} onClick={() => onBulkPause(selectedTasks())} />
                <BulkMenuItem label={t("taskList.bulkResume")} onClick={() => onBulkResume(selectedTasks())} />
                <BulkMenuItem label={t("taskList.bulkRetry")} onClick={() => onBulkRetry(selectedTasks())} />
                <div className="my-1 h-px bg-border-subtle" aria-hidden />
                <BulkMenuItem label={t("taskList.bulkOpenFolder")} onClick={() => onBulkOpenFolder(selectedTasks())} />
                <BulkMenuItem
                  label={t("taskList.selectVisible", { count: filtered.length })}
                  onClick={() => setSelectedIds(filtered)}
                  disabled={allVisibleSelected}
                />
                <BulkMenuItem label={t("taskList.exportJson")} onClick={() => onBulkExport(selectedTasks(), "json")} />
                <BulkMenuItem label={t("taskList.exportCsv")} onClick={() => onBulkExport(selectedTasks(), "csv")} />
                {onBulkDeleteFiles ? (
                  <>
                    <div className="my-1 h-px bg-border-subtle" aria-hidden />
                    <BulkMenuItem
                      label={t("deleteDialog.deleteFilesToo")}
                      onClick={() => onBulkDeleteFiles(selectedTasks())}
                      destructive
                    />
                  </>
                ) : null}
              </div>
            </PopoverContent>
          </Popover>
          <Button
            type="button"
            variant="danger"
            size="sm"
            className="h-11 md:h-8"
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
            className="px-1.5 text-[11px] text-text-muted"
            onClick={() => setFilters({ fileType: "all", source: "all", failure: "all", resume: "all" })}
          >
            {t("taskList.clearAllFilters")}
          </Button>
        </div>
      ) : null}

      <div className="border-b border-border-subtle bg-surface-base/70 px-3 py-2 text-xs">
        <div className="flex min-w-0 items-center gap-1">
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
            className="min-w-0 text-text-muted"
          >
            <SlidersHorizontal className="h-4 w-4 shrink-0" aria-hidden="true" />
            <span className="truncate">{t("taskList.toolPanel")}</span>
            <ChevronDown
              className={`h-4 w-4 shrink-0 transition-transform duration-ui ${toolPanelOpen ? "rotate-180" : ""}`}
              aria-hidden="true"
            />
          </Button>
        </div>
        {toolPanelOpen ? (
          <div id="task-list-tool-panel" className="mt-2 grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-4">
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

      <ListContextMenu
        onNewDownload={onNewDownload}
        onPasteAndCreate={onPasteAndCreate}
        onSelectAll={() => setSelectedIds(filtered)}
        onClearSelection={selectedIds.length > 0 ? clearSelectedIds : undefined}
        onRefresh={onRefresh}
        onExport={selectedIds.length > 0 ? (format) => onBulkExport(selectedTasks(), format) : undefined}
        hasSelection={selectedIds.length > 0}
      >
        <div ref={scrollContainerRef} className="min-h-0 flex-1 overflow-y-auto">
          {loading && !initialLoadDoneRef.current ? (
            <TaskListLoadingSkeleton label={t("taskList.loading")} />
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-4 px-6 py-20 text-center">
              <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-accent-primary/8">
                {search || activeFilterCount > 0 ? (
                  <Search className="h-7 w-7 text-text-muted" />
                ) : (
                  <Plus className="h-7 w-7 text-accent-primary/70" />
                )}
              </div>
              <div className="space-y-1.5">
                <p className="text-sm font-medium text-text-primary">
                  {search || activeFilterCount > 0 ? t("taskList.emptySearch") : t("taskList.empty")}
                </p>
                {!(search || activeFilterCount > 0) ? (
                  <p className="max-w-xs text-xs leading-relaxed text-text-muted">{t("taskList.emptyHint")}</p>
                ) : null}
              </div>
              {activeFilterCount > 0 ? (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setFilters({ fileType: "all", source: "all", failure: "all", resume: "all" })}
                >
                  {t("taskList.clearFilters")}
                </Button>
              ) : !search ? (
                <Button type="button" size="sm" onClick={onNewDownload}>
                  <Plus className="h-4 w-4" aria-hidden />
                  {t("commandBar.newDownload")}
                </Button>
              ) : null}
            </div>
          ) : (
            <>
              {/* biome-ignore lint/a11y/useSemanticElements: Virtual rows require measured positioning wrappers, so explicit list semantics avoid invalid ul/div/li nesting. */}
              <div
                role="list"
                aria-label={t("taskList.aria")}
                onKeyDown={handleListKeyDown}
                className="relative [--lp:10px] sm:[--lp:12px] md:[--lp:16px] px-2.5 pt-[var(--lp)] pb-[var(--lp)] sm:px-3 md:px-4"
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
                        isShiftAnchor={selectedIds.length > 1 && taskId === selectionAnchorId}
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
                        onDeleteFiles={onDeleteFiles}
                        onResolveAttention={onResolveAttention}
                        onReorder={onReorder}
                        onCopyUrl={onCopyUrl}
                        onCopyLocalPath={onCopyLocalPath}
                        onShowDetails={onShowDetails}
                      />
                    </div>
                  );
                })}
              </div>
              {hasMore ? (
                <p className="px-2 py-3 text-center text-xs text-text-muted">{t("taskList.loadingMore")}</p>
              ) : null}
            </>
          )}
        </div>
      </ListContextMenu>
    </div>
  );
});

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
            <div className="skeleton-shimmer">
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

function SurfaceLoadingSkeleton({ label }: { label: string }) {
  return (
    <div
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root p-3 sm:p-4 md:p-6"
      role="status"
      aria-label={label}
    >
      <span className="sr-only">{label}</span>
      <div className="skeleton-shimmer mx-auto w-full max-w-4xl space-y-4">
        <div className="h-9 w-full rounded-md bg-surface-base" />
        <div className="h-8 w-3/4 rounded-md bg-surface-base" />
        <div className="space-y-3 rounded-lg border border-border-subtle/60 bg-surface-base/60 p-4">
          <div className="h-4 w-40 rounded bg-surface-raised" />
          <div className="h-10 w-full rounded bg-surface-raised/80" />
          <div className="h-10 w-full rounded bg-surface-raised/80" />
        </div>
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
    <div className="flex h-11 items-center gap-1.5 text-text-muted md:h-8">
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
    </div>
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
        className="ml-0.5 -mr-0.5 inline-flex min-h-9 min-w-9 items-center justify-center rounded-full text-text-muted transition-colors hover:text-text-primary focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:outline-none md:min-h-8 md:min-w-8"
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

function BulkMenuItem({
  label,
  onClick,
  disabled,
  destructive,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  destructive?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex h-9 w-full items-center rounded-md px-2 text-left text-sm md:h-8",
        "transition-[background-color,color] duration-[var(--motion-ui)] ease-out",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        "disabled:pointer-events-none disabled:opacity-40",
        destructive
          ? "text-status-danger hover:bg-status-danger/10 hover:text-status-danger"
          : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
      )}
    >
      {label}
    </button>
  );
}
