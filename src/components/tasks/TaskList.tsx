import { useVirtualizer } from "@tanstack/react-virtual";
import {
  CheckCircle2,
  ChevronDown,
  CircleX,
  MoreHorizontal,
  Pause,
  Play,
  Plus,
  RotateCcw,
  Search,
  SlidersHorizontal,
  TriangleAlert,
  X,
} from "lucide-react";
import { useReducedMotion } from "motion/react";
import { lazy, memo, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueueReasons } from "@/hooks/use-queue-reasons";
import { useRowSegments } from "@/hooks/use-row-segments";
import { handleMenuKeyDown } from "@/lib/menu-keyboard";

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

const QueueCenter = lazy(() =>
  import("@/components/workspaces/QueueCenter").then((m) => ({
    default: m.QueueCenter,
  })),
);

const StorageCenter = lazy(() =>
  import("@/components/workspaces/StorageCenter").then((m) => ({
    default: m.StorageCenter,
  })),
);

const RecoveryCenter = lazy(() =>
  import("@/components/workspaces/RecoveryCenter").then((m) => ({
    default: m.RecoveryCenter,
  })),
);

const BackupCenter = lazy(() =>
  import("@/components/workspaces/BackupCenter").then((m) => ({
    default: m.BackupCenter,
  })),
);

import { allowedTransferActions } from "@/components/tasks/row-recovery";
import { ListContextMenu, type ReorderAction } from "@/components/tasks/TaskContextMenu";
import { TaskRow } from "@/components/tasks/TaskRow";
import { taskRowEstimateFor } from "@/components/tasks/task-layout";
import { Button } from "@/components/ui/button";
import { LiveRegion } from "@/components/ui/live-region";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { RecoveryAction, TaskPriority } from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";
import { errorMessage } from "@/lib/errors";
import { beginListLoad, createListLoadFlight, endListLoad, isCurrentListQueryEpoch } from "@/lib/list-query-epoch";
import type { Platform } from "@/lib/platform";
import { listTasksCursor } from "@/lib/tauri";
import { cn, formatShortcut } from "@/lib/utils";
import {
  type FileTypeFilter,
  type NavFilter,
  type ResumeFilter,
  taskCursorInput,
  useTaskDataStore,
  useTaskUIStore,
} from "@/stores/task-store";
import type { Task } from "@/types/task";

/** `all` has no key — the chip falls back to an empty value for it. */
const FILE_TYPE_KEYS = {
  archive: "taskList.fileTypeArchive",
  image: "taskList.fileTypeImage",
  video: "taskList.fileTypeVideo",
  document: "taskList.fileTypeDocument",
  app: "taskList.fileTypeApp",
  other: "taskList.fileTypeOther",
} as const satisfies Record<Exclude<FileTypeFilter, "all">, TranslationKey>;

/** The three list views behind the sidebar's one "Needs you" entry. The cause
 * filter switches between them, so a failed download and one waiting on a
 * decision share a layout, search, and sort instead of living in two places. */
const ISSUE_NAVS = new Set<NavFilter>(["issues", "attention", "failed"]);

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
  platform = "unknown",
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
  /** Formats the Mod+N hint so macOS shows the Command glyph, not "Ctrl". */
  platform?: Platform;
}) {
  const { t } = useTranslation();
  const reduceMotion = !!useReducedMotion();
  const [bulkMenuOpen, setBulkMenuOpen] = useState(false);
  const [moreFiltersOpen, setMoreFiltersOpen] = useState(false);
  const [statusAnnouncement, setStatusAnnouncement] = useState("");
  const [allSearchMatches, setAllSearchMatches] = useState<number | null>(null);
  const searchMatchRequestRef = useRef(0);
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
  const rowDensity = useTaskUIStore((s) => s.rowDensity);
  const setRowDensity = useTaskUIStore((s) => s.setRowDensity);
  const compactRows = rowDensity === "compact";
  const selectedId = useTaskUIStore((s) => s.selectedId);
  const selectedIds = useTaskUIStore((s) => s.selectedIds);
  const selectionAnchorId = useTaskUIStore((s) => s.selectionAnchorId);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const setSort = useTaskUIStore((s) => s.setSort);
  const filters = useTaskUIStore((s) => s.filters);
  const pendingDeleteIds = useTaskUIStore((s) => s.pendingDeleteIds);
  const selectTask = useTaskUIStore((s) => s.selectTask);
  const setSelectedIds = useTaskUIStore((s) => s.setSelectedIds);
  const setTaskSelected = useTaskUIStore((s) => s.setTaskSelected);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setFilters = useTaskUIStore((s) => s.setFilters);
  const setSearch = useTaskUIStore((s) => s.setSearch);
  const setNav = useTaskUIStore((s) => s.setNav);
  const setSelectionAnchor = useTaskUIStore((s) => s.setSelectionAnchor);
  // Primitive selector: progress ticks rebuild the stats object but rarely
  // change these three numbers, so the list does not re-render per tick.
  const issueCounts = useTaskDataStore((s) => {
    const stats = s.globalTaskStats ?? s.taskStats;
    return `${stats.attention}:${stats.failed}:${stats.all}`;
  });
  const [attentionCount, failedCount, totalTaskCount] = issueCounts.split(":").map(Number);
  const issueView = ISSUE_NAVS.has(nav);
  const toolPanelOpen = useTaskUIStore((s) => s.toolPanelOpen);
  const setToolPanelOpen = useTaskUIStore((s) => s.setToolPanelOpen);
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
  const scopeLabel = (() => {
    switch (nav) {
      case "issues":
        return t("nav.issues");
      case "attention":
        return t("nav.attention");
      case "queue":
        return t("queueCenter.title");
      case "recovery":
        return t("recoveryCenter.title");
      case "downloading":
        return t("nav.downloading");
      case "paused":
        return t("nav.paused");
      case "completed":
        return t("nav.completed");
      case "failed":
        return t("nav.failed");
      default:
        return t("nav.all");
    }
  })();
  const advancedFilterCount =
    Number(filters.source !== "all") + Number(filters.failure !== "all") + Number(filters.resume !== "all");

  // Hide tasks that are in the soft-delete undo window so the list reflects
  // the deletion immediately while the undo toast is reachable.
  const pendingDeleteSet = useMemo(() => new Set(pendingDeleteIds), [pendingDeleteIds]);
  const filtered = useMemo(
    () => (pendingDeleteSet.size === 0 ? taskIds : taskIds.filter((id) => !pendingDeleteSet.has(id))),
    [pendingDeleteSet, taskIds],
  );
  const filteredRef = useRef(filtered);
  filteredRef.current = filtered;

  // An empty scoped search should offer a route to the matching task instead
  // of leaving the user at a dead end in the current status view.
  // biome-ignore lint/correctness/useExhaustiveDependencies: taskCursorInput reads filters from the UI store; filter identity is intentionally the invalidation signal.
  useEffect(() => {
    const requestId = ++searchMatchRequestRef.current;
    if (!search.trim() || nav === "all" || filtered.length > 0) {
      setAllSearchMatches(null);
      return;
    }
    let active = true;
    void listTasksCursor(taskCursorInput(null, { nav: "all" }))
      .then((result) => {
        if (active && requestId === searchMatchRequestRef.current) setAllSearchMatches(result.minimumTotal);
      })
      .catch(() => {
        if (active && requestId === searchMatchRequestRef.current) setAllSearchMatches(null);
      });
    return () => {
      active = false;
    };
  }, [filtered.length, filters, nav, search]);

  // One shared scheduler poll for the whole list; rows read their decision from
  // the returned map instead of each fetching their own.
  const queueReasons = useQueueReasons(filtered);

  // Announce task status changes for screen readers (WCAG 4.1.3).
  // PERF-03: subscribe to statusAnnounceEpoch so progress-only patches never
  // scan the loaded task list. The store records transitions only when status changes.
  useEffect(() => {
    let lastEpoch = useTaskDataStore.getState().statusAnnounceEpoch;
    return useTaskDataStore.subscribe((state) => {
      if (state.statusAnnounceEpoch === lastEpoch) return;
      lastEpoch = state.statusAnnounceEpoch;
      const transition = state.lastStatusTransitions[state.lastStatusTransitions.length - 1];
      if (!transition) return;
      setStatusAnnouncement(
        t("taskList.statusChanged", {
          name: transition.task.fileName,
          status: t(`task.status.${transition.task.status}`),
        }),
      );
    });
  }, [t]);
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  // filters/nav/sort are read inside taskCursorInput via the UI store; listing
  // them in deps keeps loadPage identity tied to query changes so the effect below reloads.
  // biome-ignore lint/correctness/useExhaustiveDependencies: store-backed query fields must invalidate this callback
  const loadPage = useCallback(
    async (cursor: string | null, append = false) => {
      const begin = beginListLoad(loadFlightRef.current, append);
      if (begin.kind === "skip") return;
      const { epoch, role } = begin;
      if (role === "replace") setLoading(true);
      try {
        const result = await listTasksCursor(taskCursorInput(cursor, { search }));
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
    [search, filters, nav, selectTask, setError, setLoading, setTaskCursorPage, sortDirection, sortKey],
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
    estimateSize: () => taskRowEstimateFor(rowDensity), // density preset + 8px gap; failed/expanded rows are measured.
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

  // Density changes every row's height at once, so the cached measurements from
  // the previous preset have to go; otherwise the total scroll size stays wrong
  // until each row happens to re-render.
  // biome-ignore lint/correctness/useExhaustiveDependencies: rowDensity intentionally triggers this imperative virtualizer reset.
  useEffect(() => {
    virtualizer.measure();
  }, [rowDensity, virtualizer]);

  // Only rows on screen ask for their byte ranges; see useRowSegments.
  const visibleTaskIds = virtualizer
    .getVirtualItems()
    .map((item) => filtered[item.index])
    .filter((id): id is string => Boolean(id));
  const rowSegments = useRowSegments(visibleTaskIds);

  // Scroll to top when filter / sort / search changes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: query fields intentionally trigger this imperative virtualizer reset.
  useEffect(() => {
    virtualizer.scrollToOffset(0);
  }, [filters, nav, search, sortDirection, sortKey]);
  const selectedIdSet = useMemo(() => new Set(selectedIds), [selectedIds]);
  const selectedTasks = useCallback(() => {
    const { taskById } = useTaskDataStore.getState();
    return selectedIds.map((id) => taskById[id]).filter((task): task is Task => Boolean(task));
  }, [selectedIds]);
  const runAfterBulkMenuClose = useCallback((action: () => void) => {
    setBulkMenuOpen(false);
    // Native save/open dialogs do not appear if they are invoked while this
    // popover still owns focus in the Tauri webview. Close first, then run.
    window.setTimeout(action, 0);
  }, []);
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
  const selectedActionCounts = useTaskDataStore((s) => {
    let pause = 0;
    let resume = 0;
    let retry = 0;
    for (const id of selectedIds) {
      const task = s.taskById[id];
      if (!task) continue;
      const actions = allowedTransferActions(task);
      if (actions.includes("pause")) pause++;
      if (actions.includes("resume")) resume++;
      if (actions.includes("retry")) retry++;
    }
    return `${pause}:${resume}:${retry}`;
  });
  const [pauseableSelected, resumableSelected, retryableSelected] = selectedActionCounts.split(":").map(Number);
  const sourceOptions = filterOptions.sources;
  // E-3: failureOptions is read from the store to avoid depending on taskById (which rebuilds its reference every 250ms) and causing per-frame recompute.
  // Prefer backend-provided failureCategories when available; otherwise use the store-computed value.
  const failureOptions =
    filterOptions.failureCategories.length > 0 ? filterOptions.failureCategories : storeFailureOptions;

  // Declared ahead of selectAndFocus, which records the ids it has already
  // handled so the selection effect below does not scroll a second time.
  const lastScrolledSelectedIdRef = useRef<string | null>(null);

  const selectAndFocus = useCallback(
    (taskId: string, source: "pointer" | "keyboard" = "keyboard") => {
      lastScrolledSelectedIdRef.current = taskId;
      selectTask(taskId);
      // A clicked row is already on screen. Scrolling it (the old code centred
      // it) moved a different row under the cursor, so the second click of a
      // double-click, a Shift-click, or a click on an inline Restart button
      // could land on the wrong task. Keyboard moves scroll only as far as
      // needed to reveal the row, instead of re-centring on every arrow press.
      if (source === "keyboard") {
        const index = filteredRef.current.indexOf(taskId);
        if (index >= 0) {
          virtualizer.scrollToIndex(index, { align: "auto" });
        }
      }
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          document.getElementById(`task-row-${taskId}`)?.focus({ preventScroll: source === "pointer" });
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

  // UX-17: scroll the selected row into view only when the selection itself
  // changes. `filtered` gets a fresh identity on every infinite-scroll append,
  // so keeping it in the deps re-centered the viewport onto the selection after
  // each page load; the latest list is read through filteredRef instead.
  // Selections made in the list itself are marked by selectAndFocus and skip
  // this; it only centres selections that arrive from elsewhere (palette,
  // a newly created task, a workspace "show in list").
  useEffect(() => {
    // UX-17: clear on deselect so re-selecting the same task later still
    // scrolls it into view instead of being treated as an already-scrolled id.
    if (!selectedId) {
      lastScrolledSelectedIdRef.current = null;
      return;
    }
    if (lastScrolledSelectedIdRef.current === selectedId) return;
    const index = filteredRef.current.indexOf(selectedId);
    if (index < 0) return;
    lastScrolledSelectedIdRef.current = selectedId;
    virtualizer.scrollToIndex(index, { align: "center" });
  }, [selectedId, virtualizer]);

  // Shift+Arrow: move the focus one row and select everything between the
  // anchor and it, the keyboard twin of Shift+click.
  const extendSelection = useCallback(
    (direction: "next" | "prev") => {
      const list = filteredRef.current;
      if (list.length === 0) return;
      const currentId = selectedIdRef.current;
      const currentIndex = currentId ? list.indexOf(currentId) : -1;
      const startIndex = currentIndex >= 0 ? currentIndex : 0;
      const nextIndex = direction === "next" ? Math.min(list.length - 1, startIndex + 1) : Math.max(0, startIndex - 1);
      const nextId = list[nextIndex];
      if (!nextId) return;
      const storedAnchor = useTaskUIStore.getState().selectionAnchorId;
      const anchorId = storedAnchor && list.includes(storedAnchor) ? storedAnchor : (currentId ?? nextId);
      selectAndFocus(nextId);
      // selectTask moves the anchor to the focused row; a range keeps growing
      // from where it started, so the anchor is put back.
      setSelectionAnchor(anchorId);
      const anchorIndex = list.indexOf(anchorId);
      const [from, to] = anchorIndex < nextIndex ? [anchorIndex, nextIndex] : [nextIndex, anchorIndex];
      setSelectedIds(list.slice(from, to + 1));
    },
    [selectAndFocus, setSelectedIds, setSelectionAnchor],
  );

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
      // Rows handle their own arrows (and Shift+Arrow) and mark the event;
      // acting on it again here would reset a range selection's anchor.
      if (event.defaultPrevented) return;
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

  if (nav === "storage") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("storageCenter.loading")} />}>
        <StorageCenter />
      </Suspense>
    );
  }

  if (nav === "backup") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("backupCenter.loading")} />}>
        <BackupCenter />
      </Suspense>
    );
  }

  if (nav === "recovery") {
    return (
      <Suspense fallback={<SurfaceLoadingSkeleton label={t("recoveryCenter.loading")} />}>
        <RecoveryCenter
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

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      <h1 className="sr-only">{t("taskList.aria")}</h1>
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

      {/* Screen reader status announcements (outside #root, see LiveRegion). */}
      <LiveRegion>{statusAnnouncement}</LiveRegion>

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
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-11 md:h-8"
            disabled={pauseableSelected === 0}
            onClick={() => onBulkPause(selectedTasks())}
          >
            <Pause className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t("taskList.bulkPause")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-11 md:h-8"
            disabled={resumableSelected === 0}
            onClick={() => onBulkResume(selectedTasks())}
          >
            <Play className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t("taskList.bulkResume")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-11 md:h-8"
            disabled={retryableSelected === 0}
            onClick={() => onBulkRetry(selectedTasks())}
          >
            <RotateCcw className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t("taskList.bulkRetry")}
          </Button>
          <Popover open={bulkMenuOpen} onOpenChange={setBulkMenuOpen} modal={false}>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-11 md:h-8 data-[state=open]:bg-surface-raised data-[state=open]:text-text-primary"
                aria-label={t("taskList.moreBulkActions")}
                aria-haspopup="menu"
                aria-expanded={bulkMenuOpen}
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
                {t("taskList.more")}
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              className="w-52"
              onCloseAutoFocus={(event) => event.preventDefault()}
              onOpenAutoFocus={(event) => {
                event.preventDefault();
                requestAnimationFrame(() => {
                  document
                    .getElementById("task-list-bulk-menu")
                    ?.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')
                    ?.focus();
                });
              }}
            >
              <div id="task-list-bulk-menu" className="space-y-0.5" role="menu" onKeyDown={handleMenuKeyDown}>
                <BulkMenuItem
                  label={t("taskList.bulkPause")}
                  disabled={pauseableSelected === 0}
                  onClick={() => runAfterBulkMenuClose(() => onBulkPause(selectedTasks()))}
                />
                <BulkMenuItem
                  label={t("taskList.bulkResume")}
                  disabled={resumableSelected === 0}
                  onClick={() => runAfterBulkMenuClose(() => onBulkResume(selectedTasks()))}
                />
                <BulkMenuItem
                  label={t("taskList.bulkRetry")}
                  disabled={retryableSelected === 0}
                  onClick={() => runAfterBulkMenuClose(() => onBulkRetry(selectedTasks()))}
                />
                <div className="my-1 h-px bg-border-subtle" aria-hidden />
                <BulkMenuItem
                  label={t("taskList.bulkOpenFolder")}
                  onClick={() => runAfterBulkMenuClose(() => onBulkOpenFolder(selectedTasks()))}
                />
                <BulkMenuItem
                  label={t("taskList.selectVisible", { count: filtered.length })}
                  onClick={() => runAfterBulkMenuClose(() => setSelectedIds(filtered))}
                  disabled={allVisibleSelected}
                />
                <BulkMenuItem
                  label={t("taskList.exportJson")}
                  onClick={() => {
                    const tasks = selectedTasks();
                    runAfterBulkMenuClose(() => onBulkExport(tasks, "json"));
                  }}
                />
                <BulkMenuItem
                  label={t("taskList.exportCsv")}
                  onClick={() => {
                    const tasks = selectedTasks();
                    runAfterBulkMenuClose(() => onBulkExport(tasks, "csv"));
                  }}
                />
                {onBulkDeleteFiles ? (
                  <>
                    <div className="my-1 h-px bg-border-subtle" aria-hidden />
                    <BulkMenuItem
                      label={t("deleteDialog.deleteFilesToo")}
                      onClick={() => runAfterBulkMenuClose(() => onBulkDeleteFiles(selectedTasks()))}
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

      {issueView ? (
        <IssueCauseFilter
          nav={nav}
          attention={attentionCount}
          failed={failedCount}
          onChange={(next) => {
            clearSelectedIds();
            setNav(next);
          }}
        />
      ) : null}

      {search || activeFilterCount > 0 ? (
        <div className="flex min-w-0 items-center gap-1.5 border-b border-border-subtle/70 px-3 py-1.5 text-xs text-text-muted md:px-4">
          <span className="shrink-0 font-medium text-text-secondary">{scopeLabel}</span>
          {search ? (
            <span className="min-w-0 truncate" title={search}>
              · {t("commandBar.searchAria")}: <span className="text-text-primary">{search}</span>
            </span>
          ) : null}
          {activeFilterCount > 0 ? (
            <span className="shrink-0">· {t("taskList.toolPanelActive", { count: activeFilterCount })}</span>
          ) : null}
        </div>
      ) : null}

      {/* Active filter chips */}
      {activeFilterCount > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-border-subtle px-3 py-1.5">
          <FilterChip
            active={filters.fileType !== "all"}
            label={t("taskList.fileType")}
            value={filters.fileType !== "all" ? t(FILE_TYPE_KEYS[filters.fileType]) : ""}
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
            className="px-1.5 text-xs text-text-muted"
            onClick={() => setFilters({ fileType: "all", source: "all", failure: "all", resume: "all" })}
          >
            {t("taskList.clearAllFilters")}
          </Button>
        </div>
      ) : null}

      {toolPanelOpen ? (
        <div className="border-b border-border-subtle bg-surface-base/70 px-3 py-2 text-xs">
          <div className="flex min-w-0 items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              aria-expanded={toolPanelOpen}
              aria-controls="task-list-tool-panel"
              aria-label={t("taskList.hideToolPanel")}
              onClick={() => setToolPanelOpen(false)}
              className="min-w-0 text-text-muted"
            >
              <SlidersHorizontal className="h-4 w-4 shrink-0" aria-hidden="true" />
              <span className="truncate">{t("taskList.toolPanel")}</span>
              <ChevronDown
                className="h-4 w-4 shrink-0 rotate-180 transition-transform duration-ui"
                aria-hidden="true"
              />
            </Button>
          </div>
          <div id="task-list-tool-panel" className="mt-2 grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-4">
            {/* UX-12: the command bar's View menu owns Sort and row height from
                `md` up; below it the bar hides that menu, so these are the
                narrow window's only copies. One of each is visible at any
                width, and neither is a filter, so neither sits here on desktop. */}
            <div className="md:hidden">
              <SelectControl
                label={t("taskList.sort")}
                value={`${sortKey}:${sortDirection}`}
                onChange={(value) => {
                  const [key, direction] = value.split(":") as [typeof sortKey, typeof sortDirection];
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
            </div>
            <div className="md:hidden">
              <SelectControl
                label={t("taskList.rowDensity")}
                value={rowDensity}
                onChange={(value) => setRowDensity(value === "compact" ? "compact" : "comfortable")}
                options={[
                  ["comfortable", t("taskList.densityComfortable")],
                  ["compact", t("taskList.densityCompact")],
                ]}
              />
            </div>
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
            <div className="flex items-end">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 px-2 text-xs text-text-secondary"
                aria-expanded={moreFiltersOpen}
                aria-controls="task-list-more-filters"
                onClick={() => setMoreFiltersOpen((open) => !open)}
              >
                <ChevronDown
                  className={cn("mr-1.5 h-3.5 w-3.5 transition-transform", moreFiltersOpen && "rotate-180")}
                />
                {t("taskList.moreFilters")}
                {advancedFilterCount > 0 ? (
                  <span className="ml-1 rounded-full bg-accent-primary/12 px-1.5 py-0.5 text-xs leading-none text-accent-primary">
                    {advancedFilterCount}
                  </span>
                ) : null}
              </Button>
            </div>
            {moreFiltersOpen ? (
              <div
                id="task-list-more-filters"
                className="col-span-full grid min-w-0 grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3"
              >
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
        </div>
      ) : null}

      <ListContextMenu
        onNewDownload={onNewDownload}
        onPasteAndCreate={onPasteAndCreate}
        onSelectAll={() => setSelectedIds(filtered)}
        onClearSelection={selectedIds.length > 0 ? clearSelectedIds : undefined}
        onRefresh={onRefresh}
        onExport={selectedIds.length > 0 ? (format) => onBulkExport(selectedTasks(), format) : undefined}
        hasSelection={selectedIds.length > 0}
      >
        <div ref={scrollContainerRef} className="min-h-0 flex-1 overflow-y-auto bg-surface-list-well">
          {loading && !initialLoadDoneRef.current ? (
            <TaskListLoadingSkeleton label={t("taskList.loading")} />
          ) : filtered.length === 0 ? (
            <TaskListEmptyState
              kind={
                search || activeFilterCount > 0
                  ? "search"
                  : issueView
                    ? "issues"
                    : nav === "all" && totalTaskCount === 0
                      ? "firstRun"
                      : "view"
              }
              platform={platform}
              hasSearch={Boolean(search)}
              hasFilters={activeFilterCount > 0}
              onClearSearch={() => setSearch("")}
              onClearFilters={() => setFilters({ fileType: "all", source: "all", failure: "all", resume: "all" })}
              onNewDownload={onNewDownload}
              onShowAll={() => setNav("all")}
              scopeLabel={scopeLabel}
              allSearchMatches={allSearchMatches}
            />
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
                        paddingBottom: virtualRow.index < filtered.length - 1 ? 2 : 0,
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
                        onExtendSelection={extendSelection}
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
                        queueReason={queueReasons.get(taskId)}
                        segments={rowSegments.get(taskId)}
                        compact={compactRows}
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
    <div className="px-2.5 sm:px-3 md:px-4" role="status" aria-live="polite" aria-label={label}>
      <span className="sr-only">{label}</span>
      <div>
        {Array.from({ length: 5 }).map((_, index) => (
          <div key={index} className="overflow-hidden border-b border-border-subtle/70 px-2.5 py-3.5 sm:px-3 md:py-3">
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
      <span className="text-xs font-medium text-text-muted">{label}</span>
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
    <span className="inline-flex items-center gap-1 rounded-md border border-border-accent-subtle bg-accent-primary/[0.04] px-2 py-0.5 text-xs font-medium text-text-secondary">
      <span className="text-text-muted">{label}:</span>
      {value}
      <button
        type="button"
        className="ml-0.5 -mr-0.5 inline-flex min-h-9 min-w-9 items-center justify-center rounded-sm text-text-muted transition-colors hover:text-text-primary focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:outline-none md:min-h-8 md:min-w-8"
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
        "flex h-9 w-full cursor-pointer items-center rounded-md px-2 text-left text-sm md:h-8",
        "transition-[background-color,color,transform] duration-[var(--motion-ui)] ease-out",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        "disabled:pointer-events-none disabled:opacity-40",
        "active:scale-[0.99] active:duration-75",
        destructive
          ? "text-status-danger hover:bg-status-danger/10 hover:text-status-danger active:bg-status-danger/20"
          : "text-text-secondary hover:bg-surface-raised hover:text-text-primary active:bg-accent-primary/15 active:text-text-primary",
      )}
    >
      {label}
    </button>
  );
}

/**
 * Cause filter for the "Needs you" view: everything stuck, only what waits on
 * a decision, or only what failed. Each option is a list view of its own, so
 * the status bar's attention and failure chips land on the matching one.
 */
function IssueCauseFilter({
  nav,
  attention,
  failed,
  onChange,
}: {
  nav: NavFilter;
  attention: number;
  failed: number;
  onChange: (nav: NavFilter) => void;
}) {
  const { t } = useTranslation();
  const options = [
    { value: "issues", label: t("taskList.issueAll"), count: attention + failed, icon: null },
    { value: "attention", label: t("taskList.issueDecision"), count: attention, icon: TriangleAlert },
    { value: "failed", label: t("taskList.issueFailed"), count: failed, icon: CircleX },
  ] as const;
  return (
    <fieldset className="m-0 flex min-w-0 flex-wrap items-center gap-1 border-0 border-b border-border-subtle/70 px-3 py-1.5 md:px-4">
      <legend className="sr-only">{t("taskList.issueFilterAria")}</legend>
      {options.map(({ value, label, count, icon: Icon }) => {
        const active = nav === value;
        return (
          <Button
            key={value}
            type="button"
            variant="ghost"
            size="sm"
            aria-pressed={active}
            onClick={() => onChange(value)}
            className={cn(
              "h-8 gap-1.5 px-2.5 text-xs",
              active
                ? "bg-accent-primary/12 font-medium text-text-primary hover:bg-accent-primary/15"
                : "text-text-secondary",
            )}
          >
            {Icon ? (
              <Icon
                className={cn("h-3.5 w-3.5", value === "failed" ? "text-status-danger" : "text-status-warning")}
                aria-hidden
              />
            ) : null}
            {label}
            <span className="font-mono tabular-nums text-text-muted">{count}</span>
          </Button>
        );
      })}
    </fieldset>
  );
}

/**
 * Empty list copy by situation: a first run explains where downloads come
 * from, a cleared "Needs you" view reassures, and a search or filter offers
 * the way back instead of a New download button that would not help.
 */
function TaskListEmptyState({
  kind,
  platform,
  hasSearch,
  hasFilters,
  onClearSearch,
  onClearFilters,
  onNewDownload,
  onShowAll,
  scopeLabel,
  allSearchMatches,
}: {
  kind: "firstRun" | "issues" | "search" | "view";
  platform: Platform;
  hasSearch: boolean;
  hasFilters: boolean;
  onClearSearch: () => void;
  onClearFilters: () => void;
  onNewDownload: () => void;
  onShowAll: () => void;
  scopeLabel: string;
  allSearchMatches: number | null;
}) {
  const { t } = useTranslation();
  const shortcut = formatShortcut("mod+N", platform);
  const copy = {
    firstRun: { title: t("taskList.emptyFirstRun"), hint: t("taskList.emptyFirstRunHint", { shortcut }) },
    issues: { title: t("taskList.emptyIssues"), hint: t("taskList.emptyIssuesHint") },
    search: { title: t("taskList.emptySearch"), hint: null },
    view: { title: t("taskList.empty"), hint: t("taskList.emptyHint", { shortcut }) },
  }[kind];
  const Icon = kind === "search" ? Search : kind === "issues" ? CheckCircle2 : Plus;

  return (
    <div className="flex flex-col items-center justify-center gap-4 px-6 py-20 text-center">
      <Icon
        className={cn(
          "h-8 w-8",
          kind === "issues" ? "text-status-success" : kind === "search" ? "text-text-muted" : "text-accent-primary",
        )}
        aria-hidden
      />
      <div className="space-y-1.5">
        <p className="text-sm font-medium text-text-primary">{copy.title}</p>
        {kind === "search" && allSearchMatches && allSearchMatches > 0 ? (
          <p className="max-w-sm text-xs leading-relaxed text-text-muted">
            {t("taskList.emptySearchElsewhere", { view: scopeLabel, count: allSearchMatches })}
          </p>
        ) : copy.hint ? (
          <p className="max-w-sm text-xs leading-relaxed text-text-muted">{copy.hint}</p>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center justify-center gap-2">
        {hasSearch ? (
          <Button variant="outline" size="sm" onClick={onClearSearch}>
            {t("settings.clearSearch")}
          </Button>
        ) : null}
        {hasFilters ? (
          <Button variant="outline" size="sm" onClick={onClearFilters}>
            {t("taskList.clearFilters")}
          </Button>
        ) : null}
        {kind === "firstRun" || kind === "view" ? (
          <Button type="button" size="sm" onClick={onNewDownload}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("commandBar.newDownload")}
          </Button>
        ) : null}
        {kind === "issues" || kind === "view" ? (
          <Button type="button" variant="outline" size="sm" onClick={onShowAll}>
            {t("attentionCenter.viewAll")}
          </Button>
        ) : null}
        {kind === "search" && allSearchMatches && allSearchMatches > 0 ? (
          <Button type="button" variant="outline" size="sm" onClick={onShowAll}>
            {t("attentionCenter.viewAll")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
