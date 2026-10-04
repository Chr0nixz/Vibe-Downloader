import type { TFunction } from "i18next";
import {
  AlertTriangle,
  ArrowDown,
  Check,
  CircleX,
  Clock,
  Copy,
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileCog,
  FileImage,
  FileSpreadsheet,
  FileStack,
  FileText,
  FileVideo,
  FolderOpen,
  MoreHorizontal,
  PanelRight,
  Pause,
  Play,
  RotateCcw,
  ShieldAlert,
  ShieldCheck,
  Square,
  Trash2,
} from "lucide-react";
import { motion } from "motion/react";
import { type MouseEventHandler, memo, type ReactNode, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  hasInlineRecovery,
  inlineRecoveryActionsForTask,
  recoveryActionLabel,
  resumeVerdict,
  rowShowsRetry,
  rowTransferMode,
} from "@/components/tasks/row-recovery";
import { describeSpeedTrend } from "@/components/tasks/SpeedSparkline";
import { type ReorderAction, TaskContextMenu } from "@/components/tasks/TaskContextMenu";
import { recoveryActionIcon, recoveryTone, restartCost } from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Popover, PopoverClose, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { QueueTaskDecision, RecoveryAction, TaskStatus } from "@/generated/bindings";
import { useSystemFileIcon } from "@/hooks/use-system-file-icon";
import type { TranslationKey } from "@/i18n";
import { type ChunkCell, type ChunkTone, chunkCounts, chunkMapCells, interpolateChunkCells } from "@/lib/chunk-map";
import { localizedErrorCause, localizedErrorMessage, localizedMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import { cn, formatBytes, formatEta, formatPercent, formatSpeed, formatStalledSpeed } from "@/lib/utils";
import type { SpeedSample } from "@/stores/speed-history-store";
import { useSpeedHistoryStore } from "@/stores/speed-history-store";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";

interface TaskRowProps {
  taskId: string;
  selected: boolean;
  multiSelected: boolean;
  isShiftAnchor?: boolean;
  isFirstFocusable: boolean;
  reduceMotion: boolean;
  position: number;
  setSize: number;
  /** `source` tells the list whether to scroll: a pointer selection is
   * already on screen, so moving the list would put a different row under
   * the cursor for the next click. */
  onSelectTask: (taskId: string, source?: "pointer" | "keyboard") => void;
  onToggleSelected: (taskId: string, selected: boolean) => void;
  onNavigate: (direction: "next" | "prev") => void;
  /** Shift+Arrow: move the focus and grow the multi-selection from the anchor. */
  onExtendSelection?: (direction: "next" | "prev") => void;
  onShiftSelect?: (anchorId: string, currentId: string) => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onRedownload?: (task: Task) => void;
  onRecheck?: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onDelete: (task: Task) => void;
  onDeleteFiles?: (task: Task) => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onReorder?: (task: Task, action: ReorderAction) => void;
  onCopyUrl?: (task: Task) => void;
  onCopyLocalPath?: (task: Task) => void;
  onShowDetails?: (task: Task) => void;
  /** Scheduler wait decision, when this task is queued. Supplied by the list from
   * a single shared poll (`useQueueReasons`) rather than fetched per row. */
  queueReason?: QueueTaskDecision;
  /** Byte ranges from the list's shared segment poll (`useRowSegments`). When
   * present and truthful the bar is drawn as those ranges; otherwise it falls
   * back to one fill. */
  segments?: readonly TaskSegment[];
  /** Compact density preset: drops the host/diagnostic lines to sr-only, uses a
   * 2px bar and a single-line rail, and reveals checkbox/actions on hover or
   * focus. Distinct from `shellCompact`, which is a viewport-width tier. */
  compact: boolean;
}

const EMPTY_SPEED_HISTORY: SpeedSample[] = [];

/** Live numbers keep one decimal so the rail does not change width each tick. */
const FIXED = { fixed: true } as const;

/** Backend health keys can lag a status transition by one progress tick. */
const HEALTH_STATUS_HINTS: Record<string, Task["status"][]> = {
  "taskDiagnostics.idle": ["paused", "queued", "waiting_network"],
  "taskDiagnostics.downloading": ["downloading"],
  "taskDiagnostics.downloadingSteadily": ["downloading"],
  "taskDiagnostics.serverLimitDetected": ["downloading"],
  "taskDiagnostics.networkRetrying": ["retrying"],
  "taskDiagnostics.completed": ["completed"],
  "taskDiagnostics.queued": ["queued"],
  "taskDiagnostics.waitingNetwork": ["waiting_network"],
  "taskDiagnostics.finishingHls": ["downloading", "retrying"],
};

function healthSummaryMatchesStatus(summary: string | null | undefined, status: Task["status"]): boolean {
  const hints = summary ? HEALTH_STATUS_HINTS[summary] : undefined;
  return !hints || hints.includes(status);
}

function statusBadge(status: Task["status"]): string {
  switch (status) {
    // A healthy transfer stays neutral: its bar and its speed already wear the
    // accent, and a third accent pill made every busy row read as an alert.
    case "downloading":
      return "bg-surface-hover text-text-primary";
    // Moving but fighting errors is the state a glance has to catch, so it
    // takes the warning tint instead of looking like a healthy download.
    case "retrying":
      return "bg-status-warning/15 dark:bg-status-warning/10 text-status-warning";
    case "completed":
      return "bg-status-success/15 dark:bg-status-success/10 text-status-success";
    case "failed":
      return "bg-status-danger/15 dark:bg-status-danger/10 text-status-danger";
    case "needs_attention":
      return "bg-status-warning/15 dark:bg-status-warning/10 text-status-warning";
    case "paused":
      return "text-text-muted";
    case "queued":
    case "waiting_network":
      return "text-text-secondary";
    default:
      return "text-text-secondary";
  }
}

// Per-status icon: shape differentiation on top of color, so badges read at a
// glance even when hues are close (paused vs. queued, or a warm accent theme
// next to warning). Nothing spins: the bar already shows that bytes move.
function statusBadgeIcon(
  status: Task["status"],
): { Icon: React.ComponentType<{ className?: string }>; className?: string } | null {
  switch (status) {
    case "downloading":
      return { Icon: ArrowDown, className: "text-accent-primary" };
    case "retrying":
      return { Icon: RotateCcw };
    case "completed":
      return { Icon: Check };
    // Same glyphs as the sidebar's Needs you entry, so a row and the view that
    // lists it read as the same state.
    case "failed":
      return { Icon: CircleX };
    case "needs_attention":
      return { Icon: AlertTriangle };
    case "paused":
      return { Icon: Pause };
    case "queued":
    case "waiting_network":
      return { Icon: Clock };
    default:
      return null;
  }
}

/**
 * Whether the bytes already on disk survive a pause, stated on the row once
 * there is progress to protect. A server without Range support turns a pause
 * into a restart, which is the one fact a user checking a multi-GB download
 * needs before pressing Pause, so that case wears the warning tone.
 */
function resumeMark(task: Task): { key: TranslationKey; warning: boolean } | null {
  if (task.downloadedBytes <= 0) return null;
  switch (task.status) {
    case "downloading":
    case "retrying":
    case "queued":
      return task.supportsResume
        ? { key: "task.trust.resumable", warning: false }
        : { key: "task.trust.pauseRestarts", warning: true };
    case "paused":
    case "waiting_network":
      return resumeVerdict(task) === "available"
        ? { key: "task.trust.resumable", warning: false }
        : { key: "task.trust.restartRequired", warning: true };
    default:
      return null;
  }
}

// Leading file-type icon for the row's identity marker.
// Replaces the old 2-letter protocol monogram with an icon that answers
// "what kind of file is this?" at a glance — a download manager's core
// affordance. Protocol is preserved in the chip tooltip.
// Shape carries the categorization; color stays neutral to keep the row calm
// and to avoid clashing with the 8 user-selectable accent themes.
type IconComponent = React.ComponentType<{ className?: string }>;

const VIDEO_EXTS = new Set([
  "mp4",
  "mkv",
  "avi",
  "mov",
  "wmv",
  "flv",
  "webm",
  "m4v",
  "mpg",
  "mpeg",
  "ts",
  "m2ts",
  "vob",
  "3gp",
  "rm",
  "rmvb",
  "ogv",
]);
const AUDIO_EXTS = new Set([
  "mp3",
  "wav",
  "flac",
  "aac",
  "ogg",
  "opus",
  "m4a",
  "wma",
  "aiff",
  "alac",
  "ape",
  "mka",
  "ac3",
  "amr",
]);
const IMAGE_EXTS = new Set([
  "jpg",
  "jpeg",
  "png",
  "gif",
  "webp",
  "bmp",
  "svg",
  "tiff",
  "tif",
  "ico",
  "heic",
  "heif",
  "raw",
  "psd",
  "ai",
  "avif",
  "jfif",
]);
const ARCHIVE_EXTS = new Set([
  "zip",
  "rar",
  "7z",
  "tar",
  "gz",
  "bz2",
  "xz",
  "zst",
  "lz",
  "sit",
  "cab",
  "tgz",
  "tbz2",
  "txz",
]);
const DOC_EXTS = new Set([
  "pdf",
  "doc",
  "docx",
  "txt",
  "rtf",
  "odt",
  "pages",
  "md",
  "markdown",
  "epub",
  "mobi",
  "azw",
  "azw3",
  "djvu",
  "tex",
]);
const SHEET_EXTS = new Set(["xls", "xlsx", "csv", "ods", "tsv", "numbers"]);
const CODE_EXTS = new Set([
  "js",
  "ts",
  "jsx",
  "tsx",
  "py",
  "rs",
  "go",
  "java",
  "c",
  "cpp",
  "cc",
  "h",
  "hpp",
  "cs",
  "rb",
  "php",
  "swift",
  "kt",
  "kts",
  "sh",
  "bash",
  "zsh",
  "json",
  "xml",
  "yaml",
  "yml",
  "html",
  "htm",
  "css",
  "scss",
  "sass",
  "less",
  "sql",
  "lua",
  "pl",
  "r",
  "dart",
  "vue",
  "svelte",
  "toml",
  "ini",
  "cfg",
  "conf",
]);
const APP_EXTS = new Set([
  "exe",
  "msi",
  "app",
  "apk",
  "deb",
  "rpm",
  "pkg",
  "appimage",
  "dmg",
  "xpi",
  "crx",
  "jar",
  "war",
  "bin",
  "iso",
  "img",
  "vhd",
  "vhdx",
  "qcow2",
]);
const MANIFEST_EXTS = new Set(["torrent", "meta4", "metalink", "mpd", "m3u", "m3u8"]);

function fileTypeIconFor(fileName: string, protocol: string): { Icon: IconComponent; labelKey: TranslationKey } {
  const p = protocol.toLowerCase();
  // Protocol-based shortcuts: streaming media and P2P have no single extension.
  if (p === "bt" || p === "magnet") return { Icon: FileStack, labelKey: "task.fileType.torrent" };
  if (p === "hls" || p === "dash") return { Icon: FileVideo, labelKey: "task.fileType.video" };
  if (p === "metalink") return { Icon: FileText, labelKey: "task.fileType.manifest" };

  const dot = fileName.lastIndexOf(".");
  const ext = dot >= 0 ? fileName.slice(dot + 1).toLowerCase() : "";

  if (MANIFEST_EXTS.has(ext)) return { Icon: FileText, labelKey: "task.fileType.manifest" };
  if (VIDEO_EXTS.has(ext)) return { Icon: FileVideo, labelKey: "task.fileType.video" };
  if (AUDIO_EXTS.has(ext)) return { Icon: FileAudio, labelKey: "task.fileType.audio" };
  if (IMAGE_EXTS.has(ext)) return { Icon: FileImage, labelKey: "task.fileType.image" };
  if (ARCHIVE_EXTS.has(ext)) return { Icon: FileArchive, labelKey: "task.fileType.archive" };
  if (SHEET_EXTS.has(ext)) return { Icon: FileSpreadsheet, labelKey: "task.fileType.spreadsheet" };
  if (CODE_EXTS.has(ext)) return { Icon: FileCode, labelKey: "task.fileType.code" };
  if (DOC_EXTS.has(ext)) return { Icon: FileText, labelKey: "task.fileType.document" };
  if (APP_EXTS.has(ext)) return { Icon: FileCog, labelKey: "task.fileType.app" };

  return { Icon: File, labelKey: "task.fileType.file" };
}

export const TaskRow = memo(function TaskRow({
  taskId,
  selected,
  multiSelected,
  isShiftAnchor,
  isFirstFocusable,
  reduceMotion,
  position,
  setSize,
  onSelectTask,
  onToggleSelected,
  onNavigate,
  onExtendSelection,
  onShiftSelect,
  onToggleTransfer,
  onRetry,
  onRedownload,
  onRecheck,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onDelete,
  onDeleteFiles,
  onResolveAttention,
  onReorder,
  onCopyUrl,
  onCopyLocalPath,
  onShowDetails,
  queueReason,
  segments,
  compact,
}: TaskRowProps) {
  const { t } = useTranslation();
  const task = useTaskDataStore((s) => s.taskById[taskId]);
  const completionFlash = useTaskDataStore((s) => s.completionFlashIds.includes(taskId));
  const speedHistory = useSpeedHistoryStore((s) => s.history[taskId] ?? EMPTY_SPEED_HISTORY);
  // System file icon — resolved from the OS file association via IPC.
  // Called before the `if (!task)` guard would violate the Rules of Hooks,
  // so we pass the file name defensively (empty string yields null safely).
  const systemIcon = useSystemFileIcon(task?.fileName ?? "");
  const onSelect = useCallback(
    (source: "pointer" | "keyboard") => {
      onSelectTask(taskId, source);
    },
    [onSelectTask, taskId],
  );
  const speedTrend = useMemo(
    () => describeSpeedTrend(speedHistory, task?.speedBps ?? 0, t),
    [speedHistory, task?.speedBps, t],
  );
  if (!task) return null;
  const isActive = task.status === "downloading" || task.status === "retrying";
  const bytesComplete = task.totalSize > 0 && task.downloadedBytes >= task.totalSize;
  const isFinalizing = isActive && bytesComplete;
  const incompleteBytes = task.totalSize > 0 && task.downloadedBytes < task.totalSize;
  // A one-decimal label and an integer progressbar can round 99.5% up to 100%.
  // Keep unfinished transfers visibly below completion until all bytes arrive.
  const progress =
    task.totalSize > 0 ? Math.min(task.downloadedBytes / task.totalSize, incompleteBytes ? 0.994 : 1) : 0;
  const progressPercent = formatPercent(task.downloadedBytes, task.totalSize, incompleteBytes ? 99.4 : 100, FIXED);
  // Roving tab stop: only the focused row's own controls sit in the Tab order.
  // Every control stayed tabbable before, so crossing ten rows took ~50 Tab
  // presses; arrows move between rows, Tab now moves within the current one.
  const controlTabIndex = selected || isFirstFocusable ? undefined : -1;
  const retryLaterLabel =
    task.retryAfterAt && task.status === "queued"
      ? t("task.retryAfter", { time: formatClockTime(task.retryAfterAt) })
      : null;
  const healthSummary = localizedMessage(task.healthSummary, t);
  // One localized wait reason shared by the badge tooltip and the diagnostic
  // line, so the two can never disagree about why a task is queued.
  const queueReasonLabel =
    task.status === "queued" && queueReason
      ? t(`queueCenter.reason.${queueReason.reason}`, { time: formatClockTime(task.retryAfterAt ?? "") })
      : null;
  // The badge already names the state, so this line — the row's only free-form
  // text slot — has to carry the *next* useful fact. Echoing the badge here
  // wasted the one place a row could explain itself.
  const statusFact = (() => {
    switch (task.status) {
      case "downloading":
        return isFinalizing ? t("task.diagnostic.finishing") : speedTrend.label;
      case "retrying":
        return isFinalizing ? t("task.diagnostic.finishing") : t("task.diagnostic.retrying");
      case "paused":
        return t("task.diagnostic.pausedAt", { percent: progressPercent });
      case "queued":
        // `retryAfterAt` is the task's own record; the scheduler decision is the
        // authoritative reason and covers slot/host/window waits too.
        return queueReasonLabel ?? t("task.diagnostic.queuedWaiting");
      case "waiting_network":
        return t("task.diagnostic.waitingNetwork");
      case "completed": {
        const completedTime = task.completedAt
          ? formatClockTime(task.completedAt)
          : t("task.diagnostic.timeUnavailable");
        const hashTime = task.hashVerifiedAt ? formatClockTime(task.hashVerifiedAt) : completedTime;
        if (task.hashStatus === "verified") return t("task.diagnostic.checksumVerified", { time: hashTime });
        if (task.hashStatus === "failed") return t("task.diagnostic.checksumFailed", { time: hashTime });
        if (task.hashStatus === "pending") return t("task.diagnostic.checksumPending", { time: hashTime });
        return t("task.diagnostic.completedAt", { time: completedTime });
      }
      case "failed":
      case "needs_attention":
        // Reached only when there is no errorMessage to show; still more useful
        // than repeating "Failed".
        return t("task.diagnostic.stoppedAt", { percent: progressPercent });
      default:
        // Exhaustive over TaskStatus today; the cast keeps this compiling if a
        // new status ships before its diagnostic copy does.
        return t(`task.status.${task.status as TaskStatus}`);
    }
  })();
  // The badge already names the state, and the recovery banner already carries
  // message + cause for recoverable failures — a diagnostic line that only
  // repeats one of them wastes the row's single free-text slot.
  const badgeLabel = isFinalizing ? t("task.status.finishing") : t(`task.status.${task.status}`);
  // A health summary that only restates the badge ("Stream idle")
  // yields to the status fact, which says what happens next.
  const usefulHealth =
    healthSummary && healthSummary !== badgeLabel && healthSummaryMatchesStatus(task.healthSummary, task.status)
      ? healthSummary
      : null;
  const diagnosticLabel = task.errorMessage
    ? localizedErrorMessage(task.errorMessage, t)
    : isFinalizing
      ? statusFact
      : retryLaterLabel || usefulHealth || statusFact;
  const showDiagnostic = diagnosticLabel !== badgeLabel && !hasInlineRecovery(task);
  const diagnosticWarning =
    !isFinalizing && (task.status === "retrying" || (speedTrend.tone === "warning" && !task.healthSummary));
  const mark = resumeMark(task);
  const baseId = `task-${task.id}`;
  const nameId = `${baseId}-name`;
  const statusId = `${baseId}-status`;
  const hostId = `${baseId}-host`;
  const diagnosticId = `${baseId}-diagnostic`;
  const progressLabel = t("task.progressAria", {
    name: task.fileName,
    percent: progressPercent,
  });

  return (
    <TaskContextMenu
      task={task}
      onToggleTransfer={onToggleTransfer}
      onRetry={onRetry}
      onRedownload={onRedownload}
      onRecheck={onRecheck}
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
      onContextMenu={() => onSelect("pointer")}
    >
      {/* biome-ignore lint/a11y/useSemanticElements: The virtualizer inserts a measured div between the list and each row, so an explicit listitem role preserves the accessibility tree. */}
      <div
        id={`task-row-${task.id}`}
        role="listitem"
        aria-current={selected ? "true" : undefined}
        aria-posinset={position}
        aria-setsize={setSize}
        aria-labelledby={nameId}
        aria-describedby={showDiagnostic ? `${statusId} ${hostId} ${diagnosticId}` : `${statusId} ${hostId}`}
        tabIndex={selected || isFirstFocusable ? 0 : -1}
        onClick={(event) => {
          if ((event.target as HTMLElement).closest("[data-row-action]")) return;
          if (event.shiftKey && onShiftSelect) {
            const anchorId = useTaskUIStore.getState().selectionAnchorId;
            if (anchorId) {
              onShiftSelect(anchorId, taskId);
              return;
            }
          }
          onSelect("pointer");
        }}
        onDoubleClick={(event) => {
          if ((event.target as HTMLElement).closest("[data-row-action]")) return;
          if (task.status === "completed") {
            onOpenFile(task);
            return;
          }
          // Unfinished work has nothing to open yet; the double-click lands on
          // the evidence instead (chunk map, timeline, recovery), which was
          // otherwise reachable only through Enter or the context menu.
          onShowDetails?.(task);
        }}
        onKeyDown={(event) => {
          if ((event.target as HTMLElement).closest("[data-row-action]")) return;

          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            const direction = event.key === "ArrowDown" ? "next" : "prev";
            // Shift+Arrow grows the selection the way Shift+click does, so a
            // subset can be picked without leaving the keyboard.
            if (event.shiftKey && onExtendSelection) {
              onExtendSelection(direction);
              return;
            }
            onNavigate(direction);
            return;
          }
          if (event.key === "Enter") {
            event.preventDefault();
            onShowDetails?.(task);
            return;
          }
          if (event.key === " ") {
            event.preventDefault();
            // Ctrl/⌘+Space toggles this row in the multi-selection (the
            // Windows list convention); plain Space only moves the focus.
            if (event.ctrlKey || event.metaKey) {
              onToggleSelected(task.id, !multiSelected);
              return;
            }
            onSelect("keyboard");
          }
        }}
        className={cn(
          // Normal rows stay quiet and aligned; stateful rows add only a tint and
          // inset focus ring so the list scans as one surface instead of a stack of cards.
          "group relative overflow-hidden border-b border-border-subtle/70 bg-transparent transition-[background-color,border-color] duration-ui ease-out hover:bg-surface-hover/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary sm:px-3",
          compact ? "px-2.5 py-1" : "px-2.5 py-2 md:py-1.5",
          "grid gap-x-3 md:grid-cols-[minmax(0,1fr)_minmax(12rem,14rem)]",
          // Compact parks the actions beside the content from `sm` up, the same
          // trick the desktop rail uses. Below `sm` there is no room for both a
          // readable filename and three 36px touch targets, so it stays stacked.
          compact && "sm:grid-cols-[minmax(0,1fr)_auto]",
          compact ? "gap-y-1" : "gap-y-2",
          completionFlash && "completion-flash",
          // Selected: stronger accent fill + inset accent ring so the row anchors.
          selected &&
            "border-border-accent bg-accent-primary/10 shadow-[inset_0_1px_0_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]",
          multiSelected && !selected && "border-border-accent-subtle bg-accent-primary/[0.06]",
          // Shift-select anchor: bump the tint so users can see the range origin.
          isShiftAnchor && (selected || multiSelected) && "bg-accent-primary/[0.14]",
          task.status === "failed" && !selected && "border-border-danger-subtle",
          task.status === "needs_attention" && !selected && "border-border-warning-subtle",
        )}
      >
        <div className="flex min-w-0 gap-2.5">
          <label
            htmlFor={`task-select-${task.id}`}
            className={cn(
              "flex shrink-0 items-center justify-center rounded transition-opacity duration-ui hover:bg-surface-hover",
              "h-11 w-11 md:h-8 md:w-8",
              // Compact keeps the column reserved so toggling density never shifts the
              // filename horizontally; it just fades the control in on hover/focus.
              compact && !multiSelected
                ? "md:opacity-0 md:group-hover:opacity-100 md:group-focus-within:opacity-100"
                : undefined,
              compact ? "mt-0" : "mt-0.5",
            )}
            data-row-action
          >
            <span className="sr-only">{t("taskList.selectTask", { name: task.fileName })}</span>
            <Checkbox
              id={`task-select-${task.id}`}
              checked={multiSelected}
              tabIndex={controlTabIndex}
              onChange={(event) => onToggleSelected(task.id, event.target.checked)}
            />
          </label>
          {/* File-type icon — leading identity marker.
              Shows the OS-associated icon for the file's extension (e.g. the
              PDF reader's icon for .pdf, the video player's icon for .mp4),
              extracted via Windows SHGetFileInfo. Falls back to a lucide
              file-type icon when the system icon is unavailable (non-Windows,
              extraction failure, or still loading). Protocol is preserved in
              the tooltip. */}
          {(() => {
            const { Icon, labelKey } = fileTypeIconFor(task.fileName, task.protocol);
            return (
              <span
                aria-hidden
                title={`${t(labelKey)} · ${task.protocol.toUpperCase()}`}
                className="flex shrink-0 select-none items-center justify-center text-text-secondary transition-colors duration-ui group-hover:text-text-primary"
              >
                {systemIcon ? (
                  <img
                    src={systemIcon}
                    alt=""
                    className={cn("object-contain", compact ? "h-8 w-8 md:h-7 md:w-7" : "h-11 w-11 md:h-8 md:w-8")}
                    draggable={false}
                  />
                ) : (
                  <Icon className={compact ? "h-7 w-7 md:h-6 md:w-6" : "h-10 w-10 md:h-7 md:w-7"} />
                )}
              </span>
            );
          })()}
          <div className={cn("min-w-0 flex-1", compact ? "space-y-1" : "space-y-1.5 md:space-y-1")}>
            <div className="flex min-w-0 items-start justify-between gap-2 md:block">
              <div className="min-w-0 flex-1">
                {/* Compact keeps name + badge on one unwrapped line; comfortable lets
                    the badge drop below a long filename rather than squeezing it. */}
                <div className={cn("flex items-center gap-x-2", compact ? "gap-y-0.5" : "flex-wrap gap-y-1")}>
                  <div
                    id={nameId}
                    dir="auto"
                    className="truncate text-sm font-semibold leading-snug text-text-primary"
                    title={task.fileName}
                  >
                    {task.fileName}
                  </div>
                  {(() => {
                    const badgeIcon = statusBadgeIcon(task.status);
                    return (
                      <motion.span
                        id={statusId}
                        title={queueReasonLabel ?? undefined}
                        initial={reduceMotion ? false : { opacity: 0, scale: 0.96 }}
                        animate={{ opacity: 1, scale: 1 }}
                        transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
                        className={cn(
                          "inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-xs font-semibold leading-none",
                          statusBadge(task.status),
                        )}
                      >
                        {badgeIcon ? (
                          <badgeIcon.Icon className={cn("h-3 w-3 shrink-0", badgeIcon.className)} aria-hidden />
                        ) : null}
                        {badgeLabel}
                      </motion.span>
                    );
                  })()}
                </div>
                {/* Source, then whether the bytes on disk survive a pause: the
                    trust fact a user checks before pausing a long download. */}
                <p id={hostId} className="flex min-w-0 items-center gap-1.5 text-xs text-text-muted">
                  <span className="min-w-0 truncate" title={task.sourceKey}>
                    {task.sourceKey}
                  </span>
                  {mark ? (
                    <span
                      className={cn(
                        "inline-flex shrink-0 items-center gap-1",
                        mark.warning ? "font-medium text-status-warning" : "text-text-muted",
                      )}
                    >
                      <span aria-hidden>·</span>
                      {mark.warning ? (
                        <ShieldAlert className="h-3 w-3" aria-hidden />
                      ) : (
                        <ShieldCheck className="h-3 w-3" aria-hidden />
                      )}
                      {t(mark.key)}
                    </span>
                  ) : null}
                </p>
              </div>
            </div>

            {showDiagnostic ? (
              <p
                id={diagnosticId}
                title={diagnosticLabel}
                className={cn(
                  "truncate text-xs",
                  diagnosticWarning ? "font-medium text-status-warning" : "text-text-secondary",
                )}
              >
                {diagnosticLabel}
              </p>
            ) : null}

            {/* A completed row's bar is permanently pinned at 100% — pure
                redundancy next to the size line, so terminal rows skip it and
                spend the vertical budget on one more visible row instead. */}
            {task.status !== "completed" ? (
              <RowProgress
                task={task}
                segments={segments}
                progress={progress}
                label={progressLabel}
                isActive={isActive}
                compact={compact}
                flash={completionFlash}
              />
            ) : null}

            <TaskMeta
              task={task}
              isActive={isActive}
              percentLabel={progressPercent}
              layout="inline"
              compact={compact}
            />
          </div>
        </div>

        {/* Both action surfaces stay mounted and CSS alone decides visibility:
            the rail at `md`+, the stacked row below it. The old code unmounted
            one branch based on the JS resize tier, so any missed resize event
            left the row with no actions at all. */}
        <div className="hidden min-w-52 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-2 gap-y-1 text-right font-mono text-xs md:grid">
          <TaskMeta task={task} isActive={isActive} percentLabel={progressPercent} layout="rail" compact={compact} />
          <RowActions
            task={task}
            onToggleTransfer={onToggleTransfer}
            onRetry={onRetry}
            onFinishLiveRecording={onFinishLiveRecording}
            onOpenFile={onOpenFile}
            onOpenFolder={onOpenFolder}
            onShowDetails={onShowDetails}
            onDelete={onDelete}
            onCopyUrl={onCopyUrl}
            compact={compact}
            tabIndex={controlTabIndex}
            // Compact parks the actions beside the two meta lines (spanning both
            // rows) instead of giving them a row of their own — that single saved
            // row is most of the height difference between the two densities.
            className={
              compact
                ? cn(
                    "col-start-2 row-start-1 row-span-2 self-center justify-self-end",
                    // Keep the primary action path visible in compact density;
                    // hover-only controls hid pause, retry, and open-folder actions.
                    "md:transition-opacity md:duration-ui",
                  )
                : "col-span-2 justify-self-end"
            }
          />
        </div>
        <RowActions
          task={task}
          onToggleTransfer={onToggleTransfer}
          onRetry={onRetry}
          onFinishLiveRecording={onFinishLiveRecording}
          onOpenFile={onOpenFile}
          onOpenFolder={onOpenFolder}
          onShowDetails={onShowDetails}
          onDelete={onDelete}
          onCopyUrl={onCopyUrl}
          compact={compact}
          tabIndex={controlTabIndex}
          className={cn("flex md:hidden", compact && "sm:col-start-2 sm:row-start-1 sm:self-center")}
        />

        {task.status === "failed" || task.status === "needs_attention" ? (
          <InlineRecovery task={task} compact={compact} tabIndex={controlTabIndex} onResolve={onResolveAttention} />
        ) : null}
      </div>
    </TaskContextMenu>
  );
});

/// Local HH:MM for retry and completion timestamps. An unparseable (or empty)
/// value is returned as-is so `{{time}}` interpolations degrade to "" instead of
/// printing "Invalid Date".
function formatClockTime(value: string): string {
  return formatDateTime(value, "time");
}

/** Healthy ranges share the accent; only a failed range or a finished file
 * changes hue, and a paused task's ranges go gray like its bar would. */
const CHUNK_FILL: Record<ChunkTone, string> = {
  live: "bg-accent-primary",
  done: "bg-accent-primary",
  idle: "bg-progress-fill-inactive",
  failed: "bg-status-danger",
  complete: "bg-status-success",
};

/**
 * The row's progress: the byte ranges the engine is filling when the list's
 * segment poll has them, one fill otherwise. The ranges are the product's
 * signature evidence (parallel connections, where each is writing), so they
 * live in the scan path instead of three tabs deep in the details panel.
 */
const RowProgress = memo(function RowProgress({
  task,
  segments,
  progress,
  label,
  isActive,
  compact,
  flash,
}: {
  task: Task;
  segments?: readonly TaskSegment[];
  progress: number;
  label: string;
  isActive: boolean;
  compact: boolean;
  flash: boolean;
}) {
  const { t } = useTranslation();
  const cells = useMemo(() => {
    if (!segments) return null;
    const base = chunkMapCells(segments, task.totalSize, task.status);
    return base ? interpolateChunkCells(base, task.downloadedBytes) : null;
  }, [segments, task.totalSize, task.status, task.downloadedBytes]);
  const retrying = task.status === "retrying";

  if (!cells || !segments) {
    return (
      <ProgressBar
        value={progress}
        label={label}
        active={isActive}
        smooth={!isActive}
        tone={retrying ? "warning" : "primary"}
        size={compact ? "compact" : "default"}
        className={flash ? "completion-flash-progress" : undefined}
      />
    );
  }
  const percent = Math.round(progress * 100);
  return (
    <RowChunkBar
      cells={cells}
      label={label}
      percent={percent}
      summary={chunkSummaryForStatus(segments, task.status, t)}
      retrying={retrying}
      compact={compact}
    />
  );
});

function RowChunkBar({
  cells,
  label,
  percent,
  summary,
  retrying,
  compact,
}: {
  cells: readonly ChunkCell[];
  label: string;
  percent: number;
  summary: string;
  retrying: boolean;
  compact: boolean;
}) {
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${percent}% · ${summary}`}
      data-chunk-bar
      className={cn("relative overflow-hidden rounded-full bg-surface-track", compact ? "h-0.5" : "h-1.5")}
    >
      {cells.map((cell) => (
        <div
          key={cell.id}
          // The 1px seam in the track colour is what makes adjacent ranges
          // read as separate connections instead of one fill.
          className="absolute inset-y-0 overflow-hidden shadow-[inset_-1px_0_0_var(--surface-track)]"
          style={{ left: `${cell.leftPct}%`, width: `${cell.widthPct}%` }}
        >
          <div
            aria-hidden
            className={cn(
              "h-full w-full origin-left transition-transform duration-ui ease-out motion-reduce:transition-none",
              retrying && (cell.tone === "live" || cell.tone === "done") ? "bg-status-warning" : CHUNK_FILL[cell.tone],
            )}
            style={{ transform: `scaleX(${cell.fill})` }}
          />
          {cell.tone === "live" && cell.fill > 0 && cell.fill < 1 ? (
            // Write head: where this connection is writing right now. Moved by
            // transform so progress ticks never relayout the row.
            <div
              aria-hidden
              className="absolute inset-0 border-r border-text-primary/80 transition-transform duration-ui ease-out motion-reduce:transition-none"
              style={{ transform: `translateX(${(cell.fill - 1) * 100}%)` }}
            />
          ) : null}
        </div>
      ))}
    </div>
  );
}

function chunkSummaryForStatus(segments: readonly TaskSegment[], status: Task["status"], t: TFunction): string {
  const counts = chunkCounts(segments);
  // A segment poll can arrive just before the task status transition. Avoid
  // announcing "all complete" while the task itself is still transferring.
  if ((status === "downloading" || status === "retrying") && counts.total > 0 && counts.completed === counts.total) {
    return t(`task.status.${status}`);
  }
  return t("taskDetails.chunksSummary", counts);
}

// Shared meta row for speed / bytes / progress+ETA / connections.
// - `inline`: mobile single-row wrap (md:hidden).
// - `rail`: desktop right rail. Returns a fragment so the spans become direct
//   children of the parent CSS grid (placement via data-slot + col-* utilities,
//   not nth-of-type hacks).
const META_MUTED =
  "text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary";

const TaskMeta = memo(function TaskMeta({
  task,
  isActive,
  percentLabel,
  layout,
  compact,
}: {
  task: Task;
  isActive: boolean;
  percentLabel: string;
  layout: "inline" | "rail";
  compact: boolean;
}) {
  const { t } = useTranslation();
  // A stalled transfer (active, 0 B/s) is a fact worth surfacing as "0 KB/s"
  // with a warning tone; for every non-active state no speed applies, so the
  // slot is dropped entirely instead of spending the rail's most prominent
  // position on an em-dash placeholder.
  const stalled = isActive && task.speedBps <= 0;
  const speed = stalled ? formatStalledSpeed() : formatSpeed(task.speedBps, FIXED);
  const bytes = `${formatBytes(task.downloadedBytes, FIXED)} / ${formatBytes(task.totalSize, FIXED)}`;
  const eta = formatEta(task.downloadedBytes, task.totalSize, task.speedBps);
  const connections = task.connectionCount > 0 ? t("task.connections", { count: task.connectionCount }) : null;
  // A completed row states its size once — "3.9 GB / 3.9 GB" under a bar pinned
  // at 100% repeated the same fact three times on the list's most common row.
  const isCompleted = task.status === "completed";
  const sizeOnce = formatBytes(task.totalSize > 0 ? task.totalSize : task.downloadedBytes);
  const summary = [sizeOnce, connections].filter(Boolean).join(" · ");
  // Connections ride along on the progress line rather than taking a rail row of
  // their own — a rail row costs ~20px, a third of a compact row's whole budget.
  const progress = [
    task.status === "completed" || eta === "—" ? percentLabel : `${percentLabel} · ${t("task.eta")} ${eta}`,
    connections,
  ]
    .filter(Boolean)
    .join(" · ");
  // A retrying transfer's speed is real but unhealthy, so it drops the accent.
  const speedClass = stalled
    ? "font-semibold text-status-warning"
    : task.status === "retrying"
      ? "font-semibold text-text-primary"
      : "font-semibold text-accent-primary";

  if (layout === "inline") {
    return (
      <div
        className={cn(
          "flex flex-wrap items-center gap-x-2 font-mono text-xs text-text-muted md:hidden",
          compact ? "gap-y-0.5" : "gap-y-1",
        )}
      >
        {isActive ? <span className={cn("text-text-primary", speedClass)}>{speed}</span> : null}
        {isCompleted ? (
          <span className={META_MUTED} title={compact ? bytes : undefined}>
            {summary}
          </span>
        ) : (
          <>
            {compact ? null : <span className={META_MUTED}>{bytes}</span>}
            <span className={META_MUTED} title={compact ? bytes : undefined}>
              {progress}
            </span>
          </>
        )}
      </div>
    );
  }

  if (compact) {
    // Two-line rail: bytes drop to the tooltip because percent already carries
    // the progress signal, and the freed line is what lets the row hit ~48px.
    if (!isActive) {
      if (isCompleted) {
        return (
          <span data-slot="size" title={bytes} className={cn("col-start-1 row-start-1 min-w-0 truncate", META_MUTED)}>
            {summary}
          </span>
        );
      }
      // No speed applies — the progress line takes the rail's top slot.
      return (
        <span data-slot="progress" title={bytes} className={cn("col-start-1 row-start-1 min-w-0 truncate", META_MUTED)}>
          {progress}
        </span>
      );
    }
    return (
      <>
        <span data-slot="speed" className={cn("col-start-1 row-start-1 min-w-0 truncate text-sm", speedClass)}>
          {speed}
        </span>
        <span data-slot="progress" title={bytes} className={cn("col-start-1 row-start-2 min-w-0 truncate", META_MUTED)}>
          {progress}
        </span>
      </>
    );
  }

  if (!isActive) {
    if (isCompleted) {
      // Terminal rows collapse to a single rail line: the total size stated
      // once. The bar is gone and percent is always 100% here, so this line
      // is the row's only measurement.
      return (
        <span data-slot="size" className={cn("col-span-2 min-w-0 truncate", META_MUTED)}>
          {summary}
        </span>
      );
    }
    // Idle rows collapse to two rail lines: the byte count takes the
    // full-width slot the em-dash used to hold, progress stays beneath it.
    return (
      <>
        <span data-slot="bytes" className={cn("col-span-2 min-w-0 truncate", META_MUTED)}>
          {bytes}
        </span>
        <span data-slot="progress" className={cn("col-span-2 min-w-0 truncate", META_MUTED)}>
          {progress}
        </span>
      </>
    );
  }

  return (
    <>
      <span data-slot="speed" className={cn("col-start-1 min-w-0 truncate text-sm", speedClass)}>
        {speed}
      </span>
      {/* A reserved width keeps the speed's right edge still: the byte column
          used to grow and shrink with each tick and drag the speed with it. */}
      <span data-slot="bytes" className={cn("col-start-2 min-w-[17ch] truncate", META_MUTED)}>
        {bytes}
      </span>
      <span data-slot="progress" className={cn("col-span-2 min-w-0 truncate", META_MUTED)}>
        {progress}
      </span>
    </>
  );
});

function RowActions({
  task,
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onShowDetails,
  onDelete,
  onCopyUrl,
  compact,
  tabIndex,
  className,
}: {
  task: Task;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onShowDetails?: (task: Task) => void;
  onDelete: (task: Task) => void;
  onCopyUrl?: (task: Task) => void;
  compact: boolean;
  tabIndex?: number;
  className?: string;
}) {
  const { t } = useTranslation();
  const transferMode = rowTransferMode(task);
  const canFinishLiveRecording =
    task.protocol === "hls" && (task.status === "downloading" || task.status === "retrying");
  const showRetry = rowShowsRetry(task);

  return (
    <div
      className={cn(
        "flex gap-1 [&_[data-row-icon-button]]:h-10 [&_[data-row-icon-button]]:w-10 md:[&_[data-row-icon-button]]:h-8 md:[&_[data-row-icon-button]]:w-8",
        // Compact shrinks touch targets only below `md` (36px still clears WCAG
        // 2.5.8); at `md`+ it keeps the shared 32px floor — the two-line rail,
        // not the buttons, drives compact row height.
        compact && "[&_[data-row-icon-button]]:h-9 [&_[data-row-icon-button]]:w-9",
        className,
      )}
      data-row-action
      data-no-drag
    >
      {onShowDetails ? (
        <ActionButton
          label={t("contextmenu.task.showDetails")}
          ariaLabel={t("actions.showDetailsFor", { name: task.fileName })}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onShowDetails(task);
          }}
        >
          {/* The one way into the evidence (chunk map, speed history, logs):
              the old in-place expand showed a thin slice of the same panel. */}
          <PanelRight className="h-4 w-4" />
        </ActionButton>
      ) : null}
      {transferMode !== "hidden" ? (
        <ActionButton
          label={transferMode === "resume" ? t("actions.resume") : t("actions.pause")}
          ariaLabel={t(transferMode === "resume" ? "actions.resumeFor" : "actions.pauseFor", {
            name: task.fileName,
          })}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onToggleTransfer(task);
          }}
        >
          {transferMode === "resume" ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
        </ActionButton>
      ) : null}
      {showRetry ? (
        <ActionButton
          label={t("actions.retry")}
          ariaLabel={t("actions.retryFor", { name: task.fileName })}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onRetry(task);
          }}
        >
          <RotateCcw className="h-4 w-4" />
        </ActionButton>
      ) : null}
      {canFinishLiveRecording ? (
        <ActionButton
          label={t("actions.finishRecording")}
          ariaLabel={t("actions.finishRecordingFor", { name: task.fileName })}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onFinishLiveRecording(task);
          }}
        >
          <Square className="h-4 w-4" />
        </ActionButton>
      ) : null}
      {task.status === "completed" ? (
        <ActionButton
          label={t("actions.openFile")}
          ariaLabel={t("actions.openFileFor", { name: task.fileName })}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onOpenFile(task);
          }}
        >
          <File className="h-4 w-4" />
        </ActionButton>
      ) : null}
      <Popover modal={false}>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={t("actions.moreFor", { name: task.fileName })}
                title={t("taskList.more")}
                tabIndex={tabIndex}
                data-row-icon-button
                onClick={(event) => event.stopPropagation()}
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent>{t("taskList.more")}</TooltipContent>
        </Tooltip>
        <PopoverContent align="end" className="w-44 p-1" onClick={(event) => event.stopPropagation()}>
          <PopoverClose asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-9 w-full justify-start gap-2 px-2 text-sm"
              onClick={(event) => {
                event.stopPropagation();
                onOpenFolder(task);
              }}
            >
              <FolderOpen className="h-4 w-4" aria-hidden />
              {t("actions.openFolder")}
            </Button>
          </PopoverClose>
          {onCopyUrl ? (
            <PopoverClose asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-9 w-full justify-start gap-2 px-2 text-sm"
                onClick={(event) => {
                  event.stopPropagation();
                  onCopyUrl(task);
                }}
              >
                <Copy className="h-4 w-4" aria-hidden />
                {t("contextmenu.task.copyUrl")}
              </Button>
            </PopoverClose>
          ) : null}
          <PopoverClose asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-9 w-full justify-start gap-2 px-2 text-sm text-status-danger hover:bg-status-danger/10 hover:text-status-danger"
              onClick={(event) => {
                event.stopPropagation();
                onDelete(task);
              }}
            >
              <Trash2 className="h-4 w-4" aria-hidden />
              {t("deleteDialog.confirm")}
            </Button>
          </PopoverClose>
        </PopoverContent>
      </Popover>
    </div>
  );
}

const ActionButton = memo(function ActionButton({
  label,
  ariaLabel,
  disabled,
  tabIndex,
  onClick,
  children,
}: {
  label: string;
  ariaLabel?: string;
  disabled?: boolean;
  tabIndex?: number;
  onClick: MouseEventHandler<HTMLButtonElement>;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={ariaLabel ?? label}
          disabled={disabled}
          tabIndex={tabIndex}
          onClick={onClick}
          data-row-icon-button
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
});

const InlineRecovery = memo(function InlineRecovery({
  task,
  compact,
  tabIndex,
  onResolve,
}: {
  task: Task;
  compact: boolean;
  tabIndex?: number;
  onResolve: (task: Task, action: RecoveryAction) => void;
}) {
  const { t } = useTranslation();

  if (!task.errorMessage) return null;

  const recoveryActions = inlineRecoveryActionsForTask(task);

  if (recoveryActions.length === 0) return null;

  const primaryAction = recoveryActions[0];
  const moreFixes = recoveryActions.slice(1);
  const message = localizedErrorMessage(task.errorMessage, t);
  // The message names the verdict ("Cannot resume"); the cause line surfaces the
  // mechanism the backend already knows ("server dropped Range support") so the
  // user can judge whether the recovery action is safe before pressing it.
  const cause = localizedErrorCause(task.errorMessage, t);
  const tone = recoveryTone(task.status);
  const restartConsequence =
    primaryAction === "restart" || moreFixes.includes("restart")
      ? (restartCost(task, t) ?? t("recoveryDialog.restartDescription", { name: task.fileName }))
      : null;
  const restartTitle = restartConsequence ?? undefined;
  const moreFixesTitle =
    moreFixes.length > 1
      ? t("actions.moreFixesTitle", {
          fixes: moreFixes.map((action) => recoveryActionLabel(task, action, t)).join(", "),
        })
      : undefined;
  // Every button keeps the 32px height at every width (DESIGN.md minimum for
  // dense desktop UI).
  const buttonClass = "px-2 text-xs";

  return (
    // Keep the recovery context attached to the row while using a flat divider;
    // the error tint distinguishes it without creating another nested card.
    // Capped at one line (verdict · cause) plus the price of a restart, so a
    // failed row is at most one text line taller than a healthy one; the full
    // wording stays in the title and in the details panel.
    <div
      className={cn(
        "col-span-full flex flex-wrap items-center gap-x-2 gap-y-1 border-t px-2.5",
        tone === "danger"
          ? "border-border-danger-subtle bg-status-danger/[0.06]"
          : "border-border-warning-subtle bg-status-warning/[0.06]",
        compact ? "mt-0.5 py-1" : "mt-1 py-1.5",
      )}
      data-row-action
      data-no-drag
    >
      {tone === "danger" ? (
        <CircleX className="h-3.5 w-3.5 shrink-0 text-status-danger" aria-hidden />
      ) : (
        <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-status-warning" aria-hidden />
      )}
      <div className="flex min-w-[12rem] flex-1 flex-col">
        <p className="truncate text-xs leading-5" title={cause ? `${message} · ${cause}` : message}>
          <span className={cn("font-medium", tone === "danger" ? "text-status-danger" : "text-status-warning")}>
            {message}
          </span>
          {cause ? (
            <>
              <span aria-hidden className="text-text-muted">
                {" · "}
              </span>
              <span className="text-text-secondary">{cause}</span>
            </>
          ) : null}
        </p>
        {restartConsequence && !compact ? (
          <p className="truncate text-xs leading-5 text-text-primary" title={restartConsequence}>
            {restartConsequence}
          </p>
        ) : null}
        {/* The raw error code stays out of the row: it is in the details
            panel's copied report and behind "Technical details" in the
            Recovery Center. */}
      </div>
      <div className="ml-auto flex shrink-0 flex-wrap items-center gap-1">
        {/* Restart discards downloaded bytes, so it wears the same danger tint
            here as in the details panel — never the accent that marks the
            recommended action. */}
        <Button
          size="sm"
          variant={primaryAction === "restart" ? "danger" : "default"}
          className={buttonClass}
          title={primaryAction === "restart" ? restartTitle : undefined}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onResolve(task, primaryAction);
          }}
        >
          {recoveryActionLabel(task, primaryAction, t)}
        </Button>
        {moreFixes.length === 1 ? (
          // A single alternative is named on its face: a count + hover tooltip
          // forced the user to memorize the safer branch (often "save as")
          // before choosing between it and a progress-destroying restart.
          <Button
            variant="outline"
            size="sm"
            className={buttonClass}
            title={moreFixes[0] === "restart" ? restartTitle : undefined}
            tabIndex={tabIndex}
            onClick={(event) => {
              event.stopPropagation();
              onResolve(task, moreFixes[0]);
            }}
          >
            {recoveryActionLabel(task, moreFixes[0], t)}
          </Button>
        ) : moreFixes.length > 1 ? (
          // Two or more alternatives open as a menu in place; they used to
          // expand the row, which showed the save folder instead of the fixes.
          <Popover modal={false}>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className={buttonClass}
                aria-label={moreFixesTitle}
                title={moreFixesTitle}
                tabIndex={tabIndex}
                onClick={(event) => event.stopPropagation()}
              >
                {t("actions.moreFixesCount", { count: moreFixes.length })}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-52 p-1" onClick={(event) => event.stopPropagation()}>
              {moreFixes.map((action) => {
                const Icon = recoveryActionIcon(action);
                return (
                  <PopoverClose asChild key={action}>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className={cn(
                        "h-9 w-full justify-start gap-2 px-2 text-sm",
                        action === "restart" && "text-status-danger hover:bg-status-danger/10 hover:text-status-danger",
                      )}
                      title={action === "restart" ? restartTitle : undefined}
                      onClick={(event) => {
                        event.stopPropagation();
                        onResolve(task, action);
                      }}
                    >
                      <Icon className="h-4 w-4" aria-hidden />
                      {recoveryActionLabel(task, action, t)}
                    </Button>
                  </PopoverClose>
                );
              })}
            </PopoverContent>
          </Popover>
        ) : null}
      </div>
    </div>
  );
});
