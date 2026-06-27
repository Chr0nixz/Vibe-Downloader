import type { TFunction } from "i18next";
import {
  Check,
  Eye,
  EyeOff,
  File,
  Filter,
  FolderOpen,
  Gauge,
  Info,
  ListChecks,
  Moon,
  Pause,
  Play,
  Plus,
  RotateCcw,
  Search,
  Settings,
  SlidersHorizontal,
  Sun,
  Trash2,
} from "lucide-react";
import { useTheme } from "next-themes";
import { type ComponentType, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { AppSettings } from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import type { Platform } from "@/lib/platform";
import { applyGlobalSpeedLimit } from "@/lib/settings";

const log = createLogger("palette");

import { canSeedMockTasks, seedMockTasks } from "@/lib/tauri";
import { cn, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import {
  type FileTypeFilter,
  failureKind,
  filterTasks,
  type NavFilter,
  type ResumeFilter,
  type TaskFilters,
  type TaskSortDirection,
  type TaskSortKey,
  useTaskDataStore,
  useTaskUIStore,
} from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

type PaletteGroup = "app" | "task" | "bulk" | "views" | "sort" | "filters" | "speed" | "development";

interface PaletteCommand {
  id: string;
  label: string;
  description: string;
  group: PaletteGroup;
  icon?: ComponentType<{ className?: string }>;
  keywords: string[];
  shortcut?: string;
  enabled: boolean;
  disabledReason?: string;
  active?: boolean;
  featured?: boolean;
  danger?: boolean;
  run: () => void | Promise<void>;
}

const GROUP_ORDER: PaletteGroup[] = ["app", "task", "bulk", "views", "sort", "filters", "speed", "development"];

const SPEED_LIMIT_PRESETS = [
  { id: "unlimited", label: "Unlimited", value: null },
  { id: "512k", label: "512 KB/s", value: 512 * 1024 },
  { id: "1m", label: "1 MB/s", value: 1024 * 1024 },
  { id: "5m", label: "5 MB/s", value: 5 * 1024 * 1024 },
  { id: "10m", label: "10 MB/s", value: 10 * 1024 * 1024 },
] as const;

const DEFAULT_FILTERS = {
  fileType: "all" as FileTypeFilter,
  source: "all",
  failure: "all",
  resume: "all" as ResumeFilter,
};

export function Palette({
  open,
  onOpenChange,
  platform,
  selectedTask,
  onNewDownload,
  onStart,
  onPause,
  onDelete,
  onRetry,
  onOpenFile,
  onOpenFolder,
  onBulkPause,
  onBulkResume,
  onBulkRetry,
  onBulkDelete,
  onBulkOpenFolder,
  onSetNav,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  platform: Platform;
  selectedTask: Task | null;
  onNewDownload: () => void;
  onStart: () => void;
  onPause: () => void;
  onDelete: () => void;
  onRetry: () => void;
  onOpenFile: () => void;
  onOpenFolder: () => void;
  onBulkPause: (tasks: Task[]) => void;
  onBulkResume: (tasks: Task[]) => void;
  onBulkRetry: (tasks: Task[]) => void;
  onBulkDelete: (tasks: Task[]) => void;
  onBulkOpenFolder: (tasks: Task[]) => void;
  onSetNav: (nav: NavFilter) => void;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [runningId, setRunningId] = useState<string | null>(null);

  const tasks = useTaskDataStore(useShallow((s) => s.tasks));
  const selectedIds = useTaskUIStore((s) => s.selectedIds);
  const nav = useTaskUIStore((s) => s.nav);
  const taskSearch = useTaskUIStore((s) => s.search);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const filters = useTaskUIStore((s) => s.filters);
  const detailOpen = useTaskUIStore((s) => s.detailOpen);
  const setTasks = useTaskDataStore((s) => s.setTasks);
  const setError = useTaskDataStore((s) => s.setError);
  const setSelectedIds = useTaskUIStore((s) => s.setSelectedIds);
  const clearSelectedIds = useTaskUIStore((s) => s.clearSelectedIds);
  const setSort = useTaskUIStore((s) => s.setSort);
  const setFilters = useTaskUIStore((s) => s.setFilters);
  const setDetailOpen = useTaskUIStore((s) => s.setDetailOpen);

  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const addToast = useToastStore((s) => s.addToast);
  const { setTheme } = useTheme();

  // E-12: useDeferredValue 让命令面板的派生计算在空闲时跑，
  // 不阻塞主列表的进度 tick 渲染（tasks 每 250ms 变化）。
  // selectedTasks 保持实时 tasks（选中集小，需实时反馈）。
  const deferredTasks = useDeferredValue(tasks);

  const visibleTasks = useMemo(
    () => filterTasks(deferredTasks, nav, taskSearch, sortKey, sortDirection, filters),
    [deferredTasks, filters, nav, sortDirection, sortKey, taskSearch],
  );
  const selectedTasks = useMemo(() => tasks.filter((task) => selectedIds.includes(task.id)), [selectedIds, tasks]);
  const sourceOptions = useMemo(
    () => Array.from(new Set(deferredTasks.map((task) => task.sourceKey))).sort(),
    [deferredTasks],
  );
  const failureOptions = useMemo(
    () => Array.from(new Set(deferredTasks.map(failureKind).filter((kind) => kind !== "none"))).sort(),
    [deferredTasks],
  );

  const commands = useMemo(
    () =>
      buildCommands({
        t,
        platform,
        selectedTask,
        selectedTasks,
        selectedCount: selectedIds.length,
        visibleTasks,
        nav,
        sortKey,
        sortDirection,
        filters,
        sourceOptions,
        failureOptions,
        detailOpen,
        settings,
        onNewDownload,
        onStart,
        onPause,
        onDelete,
        onRetry,
        onOpenFile,
        onOpenFolder,
        onBulkPause,
        onBulkResume,
        onBulkRetry,
        onBulkDelete,
        onBulkOpenFolder,
        onSetNav,
        setSelectedIds,
        clearSelectedIds,
        setSort,
        setFilters,
        setDetailOpen,
        setSettings,
        setTasks,
        setError,
        setTheme,
      }),
    [
      clearSelectedIds,
      detailOpen,
      failureOptions,
      filters,
      nav,
      onBulkDelete,
      onBulkOpenFolder,
      onBulkPause,
      onBulkResume,
      onBulkRetry,
      onDelete,
      onNewDownload,
      onOpenFile,
      onOpenFolder,
      onPause,
      onRetry,
      onSetNav,
      onStart,
      platform,
      selectedIds.length,
      selectedTask,
      selectedTasks,
      setDetailOpen,
      setError,
      setFilters,
      setSelectedIds,
      setSettings,
      setSort,
      setTasks,
      setTheme,
      settings,
      sortDirection,
      sortKey,
      sourceOptions,
      t,
      visibleTasks,
    ],
  );

  const visibleCommands = useMemo(() => {
    const normalized = normalizeSearch(query);
    const base = normalized
      ? commands.filter((command) => commandMatches(command, normalized))
      : commands.filter((command) => command.featured !== false);

    return base.sort((a, b) => GROUP_ORDER.indexOf(a.group) - GROUP_ORDER.indexOf(b.group));
  }, [commands, query]);

  const groupedCommands = useMemo(
    () =>
      GROUP_ORDER.map((group) => ({
        group,
        commands: visibleCommands.filter((command) => command.group === group),
      })).filter((entry) => entry.commands.length > 0),
    [visibleCommands],
  );

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveIndex(0);
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  useEffect(() => {
    const firstEnabled = visibleCommands.findIndex((command) => command.enabled);
    setActiveIndex(firstEnabled >= 0 ? firstEnabled : 0);
  }, [visibleCommands]);

  useEffect(() => {
    if (listRef.current == null) return;
    const id = commandDomId(visibleCommands[activeIndex]?.id ?? "");
    const element = listRef.current.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
    element?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, visibleCommands]);

  const findEnabledIndex = useCallback(
    (from: number, direction: 1 | -1) => {
      const length = visibleCommands.length;
      if (length === 0) return -1;
      let index = from;
      for (let step = 0; step < length; step++) {
        index = (index + direction + length) % length;
        if (visibleCommands[index]?.enabled) return index;
      }
      return from;
    },
    [visibleCommands],
  );

  async function runCommand(command: PaletteCommand) {
    if (!command.enabled || runningId) return;

    try {
      setRunningId(command.id);
      await command.run();
      onOpenChange(false);
    } catch (err) {
      log.error("command execution failed", command.id, err);
      const message = errorMessage(err);
      setError(message);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: message,
      });
    } finally {
      setRunningId(null);
    }
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (visibleCommands.length === 0) {
      if (event.key === "Escape") onOpenChange(false);
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => findEnabledIndex(index, 1));
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => findEnabledIndex(index, -1));
      return;
    }
    if (event.key === "Home") {
      event.preventDefault();
      const firstEnabled = visibleCommands.findIndex((c) => c.enabled);
      if (firstEnabled >= 0) setActiveIndex(firstEnabled);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      for (let i = visibleCommands.length - 1; i >= 0; i--) {
        if (visibleCommands[i]?.enabled) {
          setActiveIndex(i);
          break;
        }
      }
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const command = visibleCommands[activeIndex];
      if (command?.enabled) void runCommand(command);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      onOpenChange(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("palette.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("palette.description")}</DialogDescription>
        </DialogHeader>
        <DialogBody className="flex flex-col overflow-hidden p-0" onKeyDown={onKeyDown}>
          <div className="shrink-0 border-b border-border-subtle p-3">
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
              <Input
                ref={inputRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t("palette.searchPlaceholder")}
                className="h-9 bg-surface-base pl-9"
                role="combobox"
                aria-expanded={open}
                aria-controls="command-palette-results"
                aria-activedescendant={
                  visibleCommands[activeIndex] ? commandDomId(visibleCommands[activeIndex].id) : undefined
                }
              />
            </div>
          </div>

          <div
            ref={listRef}
            id="command-palette-results"
            role="listbox"
            aria-label={t("palette.resultsAria")}
            className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-2"
          >
            {groupedCommands.length === 0 ? (
              <p className="px-3 py-8 text-center text-sm text-text-muted">{t("palette.noResults")}</p>
            ) : (
              groupedCommands.map((entry) => (
                <div key={entry.group} className="py-1">
                  <p className="px-2 pb-1 text-[11px] font-semibold text-text-muted">
                    {t(`palette.groups.${entry.group}`)}
                  </p>
                  <div className="space-y-1">
                    {entry.commands.map((command) => {
                      const commandIndex = visibleCommands.findIndex((entryCommand) => entryCommand.id === command.id);
                      return (
                        <CommandRow
                          key={command.id}
                          command={command}
                          active={commandIndex === activeIndex}
                          running={runningId === command.id}
                          onMouseEnter={() => setActiveIndex(commandIndex)}
                          onRun={() => void runCommand(command)}
                        />
                      );
                    })}
                  </div>
                </div>
              ))
            )}
          </div>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}

function CommandRow({
  command,
  active,
  running,
  onMouseEnter,
  onRun,
}: {
  command: PaletteCommand;
  active: boolean;
  running: boolean;
  onMouseEnter: () => void;
  onRun: () => void;
}) {
  const Icon = command.icon;
  return (
    <button
      type="button"
      id={commandDomId(command.id)}
      role="option"
      aria-selected={active}
      disabled={!command.enabled || running}
      onMouseEnter={onMouseEnter}
      onClick={onRun}
      className={cn(
        "flex w-full items-center gap-3 rounded-md px-2.5 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        active && "bg-surface-raised",
        command.enabled ? "text-text-primary hover:bg-surface-raised" : "cursor-not-allowed text-text-muted",
        command.danger && command.enabled && "text-status-danger",
      )}
    >
      <span
        className={cn(
          "flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-surface-base text-text-muted",
          active && "text-accent-primary",
          command.danger && command.enabled && "text-status-danger",
        )}
      >
        {Icon ? <Icon className="h-4 w-4" /> : null}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium">{command.label}</span>
          {command.active ? <Check className="h-3.5 w-3.5 shrink-0 text-accent-primary" /> : null}
        </span>
        <span className="block truncate text-xs text-text-muted">
          {command.enabled ? command.description : command.disabledReason}
        </span>
      </span>
      {command.shortcut ? (
        <kbd className="shrink-0 rounded border border-border-subtle px-1.5 py-0.5 font-mono text-[10px] text-text-muted">
          {command.shortcut}
        </kbd>
      ) : null}
    </button>
  );
}

function buildCommands({
  t,
  platform,
  selectedTask,
  selectedTasks,
  selectedCount,
  visibleTasks,
  nav,
  sortKey,
  sortDirection,
  filters,
  sourceOptions,
  failureOptions,
  detailOpen,
  settings,
  onNewDownload,
  onStart,
  onPause,
  onDelete,
  onRetry,
  onOpenFile,
  onOpenFolder,
  onBulkPause,
  onBulkResume,
  onBulkRetry,
  onBulkDelete,
  onBulkOpenFolder,
  onSetNav,
  setSelectedIds,
  clearSelectedIds,
  setSort,
  setFilters,
  setDetailOpen,
  setSettings,
  setTasks,
  setError,
  setTheme,
}: {
  t: TFunction;
  platform: Platform;
  selectedTask: Task | null;
  selectedTasks: Task[];
  selectedCount: number;
  visibleTasks: Task[];
  nav: NavFilter;
  sortKey: TaskSortKey;
  sortDirection: TaskSortDirection;
  filters: TaskFilters;
  sourceOptions: string[];
  failureOptions: string[];
  detailOpen: boolean;
  settings: AppSettings | null;
  onNewDownload: () => void;
  onStart: () => void;
  onPause: () => void;
  onDelete: () => void;
  onRetry: () => void;
  onOpenFile: () => void;
  onOpenFolder: () => void;
  onBulkPause: (tasks: Task[]) => void;
  onBulkResume: (tasks: Task[]) => void;
  onBulkRetry: (tasks: Task[]) => void;
  onBulkDelete: (tasks: Task[]) => void;
  onBulkOpenFolder: (tasks: Task[]) => void;
  onSetNav: (nav: NavFilter) => void;
  setSelectedIds: (ids: string[]) => void;
  clearSelectedIds: () => void;
  setSort: (key: TaskSortKey, direction?: TaskSortDirection) => void;
  setFilters: (filters: Partial<TaskFilters>) => void;
  setDetailOpen: (open: boolean) => void;
  setSettings: (settings: AppSettings) => void;
  setTasks: (tasks: Task[]) => void;
  setError: (error: string | null) => void;
  setTheme: (theme: string) => void;
}): PaletteCommand[] {
  const noTask = t("palette.disabled.noTask");
  const noSelection = t("palette.disabled.noSelection");
  const noVisibleTasks = t("palette.disabled.noVisibleTasks");
  const settingsUnavailable = t("palette.disabled.settingsUnavailable");
  const selectedName = selectedTask?.fileName ?? "";
  const currentLimit = Number(settings?.globalSpeedLimitBps ?? 0);
  const mod = platform === "macos" ? "\u2318" : "Ctrl+";

  const canStart =
    !!selectedTask &&
    (selectedTask.status === "paused" || selectedTask.status === "failed" || selectedTask.status === "waiting_network");
  const canPause =
    !!selectedTask &&
    (selectedTask.status === "downloading" || selectedTask.status === "retrying" || selectedTask.status === "queued");
  const canRetry = !!selectedTask && selectedTask.status !== "completed" && selectedTask.status !== "needs_attention";
  const canOpenFile = selectedTask?.status === "completed";
  const hasSelectedTasks = selectedTasks.length > 0;
  const canBulkPause = selectedTasks.some(
    (task) => task.status === "downloading" || task.status === "retrying" || task.status === "queued",
  );
  const canBulkResume = selectedTasks.some(
    (task) => task.status === "paused" || task.status === "failed" || task.status === "waiting_network",
  );
  const canBulkRetry = selectedTasks.some((task) => task.status !== "completed");
  const filtersActive =
    filters.fileType !== DEFAULT_FILTERS.fileType ||
    filters.source !== DEFAULT_FILTERS.source ||
    filters.failure !== DEFAULT_FILTERS.failure ||
    filters.resume !== DEFAULT_FILTERS.resume;

  const commands: PaletteCommand[] = [];
  const push = (command: PaletteCommand) => commands.push(command);
  const keyword = (...items: string[]) => items;

  push({
    id: "app.new-download",
    label: t("palette.newDownload"),
    description: t("palette.descriptions.newDownload"),
    group: "app",
    icon: Plus,
    keywords: keyword("new", "download", "add", "url", "新建", "下载", "添加"),
    shortcut: `${mod}N`,
    enabled: true,
    featured: true,
    run: onNewDownload,
  });
  push({
    id: "app.all-tasks",
    label: t("palette.commands.allTasks"),
    description: t("palette.descriptions.allTasks"),
    group: "app",
    icon: ListChecks,
    keywords: keyword("all", "tasks", "home", "全部", "任务", "主页"),
    enabled: true,
    active: nav === "all",
    featured: true,
    run: () => onSetNav("all"),
  });
  push({
    id: "app.settings",
    label: t("palette.commands.openSettings"),
    description: t("palette.descriptions.openSettings"),
    group: "app",
    icon: Settings,
    keywords: keyword("settings", "preferences", "配置", "设置"),
    shortcut: `${mod},`,
    enabled: true,
    active: nav === "settings",
    featured: true,
    run: () => onSetNav("settings"),
  });
  push({
    id: "app.about",
    label: t("palette.commands.openAbout"),
    description: t("palette.descriptions.openAbout"),
    group: "app",
    icon: Info,
    keywords: keyword("about", "version", "info", "关于", "版本", "信息"),
    enabled: true,
    active: nav === "about",
    featured: false,
    run: () => onSetNav("about"),
  });
  push({
    id: "app.theme.dark",
    label: t("palette.commands.themeDark"),
    description: t("palette.descriptions.themeDark"),
    group: "app",
    icon: Moon,
    keywords: keyword("dark", "theme", "深色", "暗夜", "模式"),
    enabled: true,
    active: false,
    run: () => setTheme("dark"),
  });
  push({
    id: "app.theme.light",
    label: t("palette.commands.themeLight"),
    description: t("palette.descriptions.themeLight"),
    group: "app",
    icon: Sun,
    keywords: keyword("light", "theme", "浅色", "亮色", "模式"),
    enabled: true,
    active: false,
    run: () => setTheme("light"),
  });

  push({
    id: "task.start",
    label: t("palette.start"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: Play,
    keywords: keyword("start", "resume", "continue", "开始", "继续"),
    enabled: canStart,
    disabledReason: selectedTask ? t("palette.disabled.cannotStart") : noTask,
    featured: true,
    run: onStart,
  });
  push({
    id: "task.pause",
    label: t("palette.pause"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: Pause,
    keywords: keyword("pause", "stop", "暂停"),
    enabled: canPause,
    disabledReason: selectedTask ? t("palette.disabled.cannotPause") : noTask,
    featured: true,
    run: onPause,
  });
  push({
    id: "task.retry",
    label: t("palette.retry"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: RotateCcw,
    keywords: keyword("retry", "again", "重试"),
    enabled: canRetry,
    disabledReason: selectedTask ? t("palette.disabled.cannotRetry") : noTask,
    featured: true,
    run: onRetry,
  });
  push({
    id: "task.open-file",
    label: t("palette.openFile"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: File,
    keywords: keyword("open", "file", "打开", "文件"),
    shortcut: `${mod}\u21B5`,
    enabled: canOpenFile,
    disabledReason: selectedTask ? t("palette.disabled.completedOnly") : noTask,
    featured: true,
    run: onOpenFile,
  });
  push({
    id: "task.open-folder",
    label: t("palette.openFolder"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: FolderOpen,
    keywords: keyword("open", "folder", "directory", "打开", "文件夹", "目录"),
    shortcut: `${mod}O`,
    enabled: !!selectedTask,
    disabledReason: noTask,
    featured: true,
    run: onOpenFolder,
  });
  push({
    id: "task.toggle-details",
    label: detailOpen ? t("palette.commands.hideDetails") : t("palette.commands.showDetails"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: detailOpen ? EyeOff : Eye,
    keywords: keyword("details", "drawer", "inspect", "详情", "详细"),
    shortcut: `${mod}D`,
    enabled: !!selectedTask,
    disabledReason: noTask,
    featured: true,
    run: () => setDetailOpen(!detailOpen),
  });
  push({
    id: "task.delete",
    label: t("palette.delete"),
    description: t("palette.descriptions.task", { name: selectedName }),
    group: "task",
    icon: Trash2,
    keywords: keyword("delete", "remove", "删除", "移除"),
    shortcut: "Del",
    enabled: !!selectedTask,
    disabledReason: noTask,
    danger: true,
    featured: true,
    run: onDelete,
  });

  push({
    id: "bulk.select-visible",
    label: t("palette.commands.selectVisible", { count: visibleTasks.length }),
    description: t("palette.descriptions.selectVisible"),
    group: "bulk",
    icon: ListChecks,
    keywords: keyword("select", "visible", "current", "选择", "当前结果"),
    shortcut: `${mod}A`,
    enabled: visibleTasks.length > 0,
    disabledReason: noVisibleTasks,
    featured: true,
    run: () => setSelectedIds(visibleTasks.map((task) => task.id)),
  });
  push({
    id: "bulk.clear-selection",
    label: t("palette.commands.clearSelection", { count: selectedCount }),
    description: t("palette.descriptions.clearSelection"),
    group: "bulk",
    icon: ListChecks,
    keywords: keyword("clear", "selection", "unselect", "清除", "取消选择"),
    shortcut: `${mod}Shift+A`,
    enabled: selectedCount > 0,
    disabledReason: noSelection,
    featured: true,
    run: clearSelectedIds,
  });
  push({
    id: "bulk.pause",
    label: t("palette.commands.bulkPause", { count: selectedTasks.length }),
    description: t("palette.descriptions.bulk", { count: selectedTasks.length }),
    group: "bulk",
    icon: Pause,
    keywords: keyword("bulk", "pause", "selected", "批量", "暂停", "选中"),
    enabled: canBulkPause,
    disabledReason: hasSelectedTasks ? t("palette.disabled.noPausableSelection") : noSelection,
    featured: true,
    run: () => onBulkPause(selectedTasks),
  });
  push({
    id: "bulk.resume",
    label: t("palette.commands.bulkResume", { count: selectedTasks.length }),
    description: t("palette.descriptions.bulk", { count: selectedTasks.length }),
    group: "bulk",
    icon: Play,
    keywords: keyword("bulk", "resume", "start", "selected", "批量", "继续"),
    enabled: canBulkResume,
    disabledReason: hasSelectedTasks ? t("palette.disabled.noResumableSelection") : noSelection,
    featured: true,
    run: () => onBulkResume(selectedTasks),
  });
  push({
    id: "bulk.retry",
    label: t("palette.commands.bulkRetry", { count: selectedTasks.length }),
    description: t("palette.descriptions.bulk", { count: selectedTasks.length }),
    group: "bulk",
    icon: RotateCcw,
    keywords: keyword("bulk", "retry", "selected", "批量", "重试"),
    enabled: canBulkRetry,
    disabledReason: hasSelectedTasks ? t("palette.disabled.noRetryableSelection") : noSelection,
    featured: true,
    run: () => onBulkRetry(selectedTasks),
  });
  push({
    id: "bulk.open-folder",
    label: t("palette.commands.bulkOpenFolder"),
    description: t("palette.descriptions.bulkOpenFolder"),
    group: "bulk",
    icon: FolderOpen,
    keywords: keyword("bulk", "open", "folder", "selected", "批量", "文件夹"),
    enabled: hasSelectedTasks,
    disabledReason: noSelection,
    featured: true,
    run: () => onBulkOpenFolder(selectedTasks),
  });
  push({
    id: "bulk.delete",
    label: t("palette.commands.bulkDelete", { count: selectedTasks.length }),
    description: t("palette.descriptions.bulk", { count: selectedTasks.length }),
    group: "bulk",
    icon: Trash2,
    keywords: keyword("bulk", "delete", "remove", "selected", "批量", "删除"),
    enabled: hasSelectedTasks,
    disabledReason: noSelection,
    featured: true,
    danger: true,
    run: () => onBulkDelete(selectedTasks),
  });

  (["all", "downloading", "paused", "completed", "failed", "settings"] as const).forEach((nextNav) => {
    push({
      id: `view.${nextNav}`,
      label: t(`nav.${nextNav}`),
      description: t("palette.descriptions.view"),
      group: "views",
      icon: nextNav === "settings" ? Settings : ListChecks,
      keywords: keyword("view", "filter", nextNav, "视图", "导航"),
      enabled: true,
      active: nav === nextNav,
      featured: nextNav !== "settings",
      run: () => onSetNav(nextNav),
    });
  });

  const sortCommands: Array<{
    id: string;
    label: string;
    key: TaskSortKey;
    direction: TaskSortDirection;
    keywords: string[];
  }> = [
    {
      id: "updated",
      label: t("taskList.sortUpdatedDesc"),
      key: "updated_at",
      direction: "desc",
      keywords: keyword("sort", "updated", "recent", "排序", "更新"),
    },
    {
      id: "created",
      label: t("taskList.sortCreatedDesc"),
      key: "created_at",
      direction: "desc",
      keywords: keyword("sort", "created", "newest", "排序", "创建"),
    },
    {
      id: "size",
      label: t("taskList.sortSizeDesc"),
      key: "file_size",
      direction: "desc",
      keywords: keyword("sort", "size", "large", "排序", "大小"),
    },
    {
      id: "progress",
      label: t("taskList.sortProgressDesc"),
      key: "progress",
      direction: "desc",
      keywords: keyword("sort", "progress", "排序", "进度"),
    },
    {
      id: "speed",
      label: t("taskList.sortSpeedDesc"),
      key: "speed",
      direction: "desc",
      keywords: keyword("sort", "speed", "排序", "速度"),
    },
    {
      id: "status",
      label: t("taskList.sortStatusAsc"),
      key: "status",
      direction: "asc",
      keywords: keyword("sort", "status", "排序", "状态"),
    },
  ];
  sortCommands.forEach((sort) => {
    push({
      id: `sort.${sort.id}`,
      label: sort.label,
      description: t("palette.descriptions.sort"),
      group: "sort",
      icon: SlidersHorizontal,
      keywords: sort.keywords,
      enabled: true,
      active: sortKey === sort.key && sortDirection === sort.direction,
      featured: false,
      run: () => setSort(sort.key, sort.direction),
    });
  });

  push({
    id: "filters.reset",
    label: t("palette.commands.resetFilters"),
    description: t("palette.descriptions.resetFilters"),
    group: "filters",
    icon: Filter,
    keywords: keyword("filter", "reset", "clear", "筛选", "重置", "清除"),
    enabled: filtersActive,
    disabledReason: t("palette.disabled.filtersAlreadyClear"),
    featured: false,
    run: () => setFilters(DEFAULT_FILTERS),
  });

  const fileTypeFilters: Array<[FileTypeFilter, string]> = [
    ["all", t("taskList.allFileTypes")],
    ["archive", t("taskList.fileTypeArchive")],
    ["image", t("taskList.fileTypeImage")],
    ["video", t("taskList.fileTypeVideo")],
    ["document", t("taskList.fileTypeDocument")],
    ["app", t("taskList.fileTypeApp")],
    ["other", t("taskList.fileTypeOther")],
  ];
  fileTypeFilters.forEach(([value, label]) => {
    push({
      id: `filter.file-type.${value}`,
      label: t("palette.commands.filterBy", { label }),
      description: t("taskList.fileType"),
      group: "filters",
      icon: Filter,
      keywords: keyword("filter", "type", value, label, "筛选", "类型"),
      enabled: true,
      active: filters.fileType === value,
      featured: false,
      run: () => setFilters({ fileType: value }),
    });
  });

  const resumeFilters: Array<[ResumeFilter, string]> = [
    ["all", t("taskList.allResume")],
    ["resumable", t("taskList.resumable")],
    ["single_connection", t("taskList.singleConnection")],
  ];
  resumeFilters.forEach(([value, label]) => {
    push({
      id: `filter.resume.${value}`,
      label: t("palette.commands.filterBy", { label }),
      description: t("taskList.resume"),
      group: "filters",
      icon: Filter,
      keywords: keyword("filter", "resume", value, label, "筛选", "续传"),
      enabled: true,
      active: filters.resume === value,
      featured: false,
      run: () => setFilters({ resume: value }),
    });
  });

  [
    ["all", t("taskList.allFailures")],
    ...failureOptions.map(
      (failure) => [failure, t(`taskList.failure_${failure}`, { defaultValue: failure })] as [string, string],
    ),
  ].forEach(([value, label]) => {
    push({
      id: `filter.failure.${value}`,
      label: t("palette.commands.filterBy", { label }),
      description: t("taskList.failure"),
      group: "filters",
      icon: Filter,
      keywords: keyword("filter", "failure", value, label, "筛选", "失败"),
      enabled: true,
      active: filters.failure === value,
      featured: false,
      run: () => setFilters({ failure: value }),
    });
  });

  [["all", t("taskList.allSources")], ...sourceOptions.map((source) => [source, source] as [string, string])].forEach(
    ([value, label]) => {
      push({
        id: `filter.source.${value}`,
        label: t("palette.commands.filterBy", { label }),
        description: t("taskList.source"),
        group: "filters",
        icon: Filter,
        keywords: keyword("filter", "source", "host", value, label, "筛选", "来源"),
        enabled: true,
        active: filters.source === value,
        featured: false,
        run: () => setFilters({ source: value }),
      });
    },
  );

  SPEED_LIMIT_PRESETS.forEach((preset) => {
    const active = preset.value === null ? currentLimit <= 0 : currentLimit === preset.value;
    push({
      id: `speed.${preset.id}`,
      label:
        preset.value === null
          ? t("speedLimit.unlimited")
          : t("palette.commands.setSpeedLimit", { speed: preset.label }),
      description:
        preset.value === null
          ? t("palette.descriptions.speedUnlimited")
          : t("palette.descriptions.speed", { speed: formatSpeed(preset.value) }),
      group: "speed",
      icon: Gauge,
      keywords: keyword("speed", "limit", "throttle", preset.label, "限速", "速度"),
      enabled: !!settings,
      disabledReason: settingsUnavailable,
      active,
      featured: preset.value === null || preset.value === 1024 * 1024,
      run: async () => {
        if (!settings) return;
        setSettings(await applyGlobalSpeedLimit(settings, preset.value));
      },
    });
  });

  if (canSeedMockTasks) {
    push({
      id: "development.reset-mock-tasks",
      label: t("palette.resetMockTasks"),
      description: t("palette.descriptions.resetMockTasks"),
      group: "development",
      icon: RotateCcw,
      keywords: keyword("debug", "mock", "seed", "reset", "开发", "模拟"),
      enabled: true,
      featured: false,
      run: async () => {
        setTasks(await seedMockTasks());
        setError(null);
      },
    });
  }

  return commands;
}

function normalizeSearch(value: string): string {
  return value.trim().toLowerCase();
}

function commandMatches(command: PaletteCommand, query: string): boolean {
  return [command.label, command.description, command.group, ...command.keywords]
    .join(" ")
    .toLowerCase()
    .includes(query);
}

function commandDomId(id: string): string {
  return `palette-command-${id.replace(/[^a-z0-9_-]/gi, "-")}`;
}
