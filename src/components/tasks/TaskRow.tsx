import {
  Activity,
  AlertTriangle,
  Check,
  ChevronDown,
  Clock,
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
  Loader2,
  Pause,
  Play,
  RotateCcw,
  Square,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type MouseEventHandler, memo, type ReactNode, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { recoveryActionsForTask, rowShowsRetry, rowTransferMode } from "@/components/tasks/row-recovery";
import { describeSpeedTrend, SpeedSparkline } from "@/components/tasks/SpeedSparkline";
import { type ReorderAction, TaskContextMenu } from "@/components/tasks/TaskContextMenu";
import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { QueueTaskDecision, RecoveryAction, TaskStatus } from "@/generated/bindings";
import { useSystemFileIcon } from "@/hooks/use-system-file-icon";
import { localizedErrorMessage, localizedMessage } from "@/lib/errors";
import { cn, formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/utils";
import type { SpeedSample } from "@/stores/speed-history-store";
import { useSpeedHistoryStore } from "@/stores/speed-history-store";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

interface TaskRowProps {
  taskId: string;
  selected: boolean;
  multiSelected: boolean;
  isShiftAnchor?: boolean;
  isFirstFocusable: boolean;
  reduceMotion: boolean;
  position: number;
  setSize: number;
  onSelectTask: (taskId: string) => void;
  onToggleSelected: (taskId: string, selected: boolean) => void;
  onNavigate: (direction: "next" | "prev") => void;
  onShiftSelect?: (anchorId: string, currentId: string) => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
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
  shellCompact: boolean;
  /** Scheduler wait decision, when this task is queued. Supplied by the list from
   * a single shared poll (`useQueueReasons`) rather than fetched per row. */
  queueReason?: QueueTaskDecision;
  /** Compact density preset: drops the host/diagnostic lines to sr-only, uses a
   * 2px bar and a single-line rail, and reveals checkbox/actions on hover or
   * focus. Distinct from `shellCompact`, which is a viewport-width tier. */
  compact: boolean;
}

const EMPTY_SPEED_HISTORY: SpeedSample[] = [];

function statusBadge(status: Task["status"]): string {
  switch (status) {
    case "downloading":
    case "retrying":
      return "bg-accent-primary/15 dark:bg-accent-primary/10 text-accent-primary";
    case "completed":
      return "bg-status-success/15 dark:bg-status-success/10 text-status-success";
    case "failed":
    case "needs_attention":
      return "bg-status-danger/15 dark:bg-status-danger/10 text-status-danger";
    case "paused":
      return "text-text-muted";
    case "queued":
    case "waiting_network":
      return "text-text-secondary";
    default:
      return "text-text-secondary";
  }
}

// Per-status icon: shape differentiation on top of color, so badges read at a glance
// even when the hue is similar (paused vs. queued were previously identical pills).
// `spin` is only true for active transfer states.
function statusBadgeIcon(
  status: Task["status"],
): { Icon: React.ComponentType<{ className?: string }>; spin: boolean } | null {
  switch (status) {
    case "downloading":
    case "retrying":
      return { Icon: Loader2, spin: true };
    case "completed":
      return { Icon: Check, spin: false };
    case "failed":
    case "needs_attention":
      return { Icon: AlertTriangle, spin: false };
    case "paused":
      return { Icon: Pause, spin: false };
    case "queued":
    case "waiting_network":
      return { Icon: Clock, spin: false };
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

function fileTypeIconFor(fileName: string, protocol: string): { Icon: IconComponent; labelKey: string } {
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
  onShiftSelect,
  onToggleTransfer,
  onRetry,
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
  shellCompact,
  queueReason,
  compact,
}: TaskRowProps) {
  const { t } = useTranslation();
  const task = useTaskDataStore((s) => s.taskById[taskId]);
  const expanded = useTaskDataStore((s) => s.expandedTaskIds.includes(taskId));
  const completionFlash = useTaskDataStore((s) => s.completionFlashIds.includes(taskId));
  const speedHistory = useSpeedHistoryStore((s) => s.history[taskId] ?? EMPTY_SPEED_HISTORY);
  const toggleTaskExpanded = useTaskDataStore((s) => s.toggleTaskExpanded);
  // System file icon — resolved from the OS file association via IPC.
  // Called before the `if (!task)` guard would violate the Rules of Hooks,
  // so we pass the file name defensively (empty string yields null safely).
  const systemIcon = useSystemFileIcon(task?.fileName ?? "");
  const onSelect = useCallback(() => {
    onSelectTask(taskId);
  }, [onSelectTask, taskId]);
  const onToggleExpanded = useCallback(() => {
    toggleTaskExpanded(taskId);
  }, [toggleTaskExpanded, taskId]);
  const speedTrend = useMemo(
    () => describeSpeedTrend(speedHistory, task?.speedBps ?? 0, t),
    [speedHistory, task?.speedBps, t],
  );
  if (!task) return null;
  const progress = task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
  const isActive = task.status === "downloading" || task.status === "retrying";
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
    const percent = formatPercent(task.downloadedBytes, task.totalSize);
    switch (task.status) {
      case "downloading":
      case "retrying":
        return speedTrend.label;
      case "paused":
        return t("task.diagnostic.pausedAt", { percent });
      case "queued":
        // `retryAfterAt` is the task's own record; the scheduler decision is the
        // authoritative reason and covers slot/host/window waits too.
        return queueReasonLabel ?? t("task.diagnostic.queuedWaiting");
      case "waiting_network":
        return t("task.diagnostic.waitingNetwork");
      case "completed": {
        const time = formatClockTime(task.updatedAt);
        if (task.hashStatus === "verified") return t("task.diagnostic.checksumVerified", { time });
        if (task.hashStatus === "failed") return t("task.diagnostic.checksumFailed", { time });
        if (task.hashStatus === "pending") return t("task.diagnostic.checksumPending", { time });
        return t("task.diagnostic.completedAt", { time });
      }
      case "failed":
      case "needs_attention":
        // Reached only when there is no errorMessage to show; still more useful
        // than repeating "Failed".
        return t("task.diagnostic.stoppedAt", { percent });
      default:
        // Exhaustive over TaskStatus today; the cast keeps this compiling if a
        // new status ships before its diagnostic copy does.
        return t(`task.status.${task.status as TaskStatus}`);
    }
  })();
  const diagnosticLabel = task.errorMessage
    ? localizedErrorMessage(task.errorMessage, t)
    : retryLaterLabel || healthSummary || statusFact;
  const baseId = `task-${task.id}`;
  const nameId = `${baseId}-name`;
  const statusId = `${baseId}-status`;
  const hostId = `${baseId}-host`;
  const diagnosticId = `${baseId}-diagnostic`;
  const expandedId = `${baseId}-expanded`;
  const progressLabel = t("task.progressAria", {
    name: task.fileName,
    percent: formatPercent(task.downloadedBytes, task.totalSize),
  });

  return (
    <TaskContextMenu
      task={task}
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
    >
      {/* biome-ignore lint/a11y/useSemanticElements: The virtualizer inserts a measured div between the list and each row, so an explicit listitem role preserves the accessibility tree. */}
      <div
        id={`task-option-${task.id}`}
        role="listitem"
        aria-current={selected ? "true" : undefined}
        aria-posinset={position}
        aria-setsize={setSize}
        aria-labelledby={nameId}
        aria-describedby={`${statusId} ${hostId} ${diagnosticId}`}
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
          onSelect();
        }}
        onDoubleClick={(event) => {
          if ((event.target as HTMLElement).closest("[data-row-action]")) return;
          if (task.status === "completed") {
            onOpenFile(task);
          }
        }}
        onKeyDown={(event) => {
          if ((event.target as HTMLElement).closest("[data-row-action]")) return;

          if (event.key === "ArrowDown") {
            event.preventDefault();
            onNavigate("next");
            return;
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            onNavigate("prev");
            return;
          }
          if (event.key === "Enter") {
            event.preventDefault();
            onShowDetails?.(task);
            return;
          }
          if (event.key === " ") {
            event.preventDefault();
            onSelect();
          }
        }}
        className={cn(
          // Row surface: a raised card on the recessed list well, with a 1px border
          // so light mode gets a real figure/ground split. Hover lifts through the
          // border + shadow rather than a bg shift — a white row has nowhere lighter
          // to go, and the shadow reads as elevation instead of a stripe.
          "group relative overflow-hidden rounded-md border border-row-border bg-surface-row transition-[background-color,border-color,box-shadow] duration-ui ease-out hover:border-row-border-hover hover:shadow-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary sm:px-3",
          compact ? "px-2.5 py-1" : "px-2.5 py-2 md:py-1.5",
          "grid gap-x-3 md:grid-cols-[minmax(0,1fr)_minmax(12rem,14rem)]",
          // Compact parks the actions beside the content from `sm` up, the same
          // trick the desktop rail uses. Below `sm` there is no room for both a
          // readable filename and three 36px touch targets, so it stays stacked.
          compact && "sm:grid-cols-[minmax(0,1fr)_auto]",
          compact ? "gap-y-1" : "gap-y-2",
          completionFlash && "completion-flash",
          // Selected: stronger accent fill (was 4%) + inset accent ring so the row anchors.
          selected &&
            "border-border-accent bg-accent-primary/10 shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_20%,transparent)]",
          multiSelected && !selected && "border-border-accent-subtle bg-accent-primary/[0.06]",
          // Shift-select anchor: bump the tint so users can see the range origin.
          isShiftAnchor && (selected || multiSelected) && "bg-accent-primary/[0.14]",
          (task.status === "failed" || task.status === "needs_attention") && !selected && "border-border-danger-subtle",
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
                          "inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[11px] font-semibold leading-none",
                          statusBadge(task.status),
                        )}
                      >
                        {badgeIcon ? (
                          <badgeIcon.Icon
                            className={cn("h-3 w-3 shrink-0", badgeIcon.spin && !reduceMotion && "animate-spin")}
                            aria-hidden
                          />
                        ) : null}
                        {t(`task.status.${task.status}`)}
                      </motion.span>
                    );
                  })()}
                </div>
                {/* Compact demotes these to sr-only rather than unmounting them:
                    aria-describedby on the row points at both ids, and the text is
                    the row's only explanation of itself. */}
                <p
                  id={hostId}
                  className={compact ? "sr-only" : "truncate text-xs text-text-muted"}
                  title={compact ? undefined : task.sourceKey}
                >
                  {task.sourceKey}
                </p>
              </div>
            </div>

            <p
              id={diagnosticId}
              title={compact ? undefined : diagnosticLabel}
              className={cn(
                compact
                  ? "sr-only"
                  : cn(
                      "truncate text-xs",
                      speedTrend.tone === "warning" && !task.healthSummary
                        ? "font-medium text-status-warning"
                        : "text-text-secondary",
                    ),
              )}
            >
              {diagnosticLabel}
            </p>

            <ProgressBar
              value={progress}
              label={progressLabel}
              active={isActive}
              smooth={!isActive}
              size={compact ? "compact" : "default"}
              className={completionFlash ? "completion-flash-progress" : undefined}
            />

            <TaskMeta task={task} isActive={isActive} layout="inline" compact={compact} />
          </div>
        </div>

        {!shellCompact ? (
          <div className="hidden min-w-52 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-2 gap-y-1 text-right font-mono text-xs md:grid">
            <TaskMeta task={task} isActive={isActive} layout="rail" compact={compact} />
            <RowActions
              task={task}
              expanded={expanded}
              expandedId={expandedId}
              onToggleExpanded={onToggleExpanded}
              onToggleTransfer={onToggleTransfer}
              onRetry={onRetry}
              onFinishLiveRecording={onFinishLiveRecording}
              onOpenFile={onOpenFile}
              onOpenFolder={onOpenFolder}
              compact={compact}
              // Compact parks the actions beside the two meta lines (spanning both
              // rows) instead of giving them a row of their own — that single saved
              // row is most of the height difference between the two densities.
              className={
                compact
                  ? cn(
                      "col-start-2 row-start-1 row-span-2 self-center justify-self-end",
                      // Fading rather than unmounting: the buttons stay in the a11y
                      // tree and tabbable, and group-focus-within reveals them for
                      // keyboard users. Hovering a button always hovers the row, so
                      // an invisible target can never be clicked by surprise.
                      "md:opacity-0 md:transition-opacity md:duration-ui md:group-hover:opacity-100 md:group-focus-within:opacity-100",
                    )
                  : "col-span-2 justify-self-end"
              }
            />
          </div>
        ) : (
          <RowActions
            task={task}
            expanded={expanded}
            expandedId={expandedId}
            onToggleExpanded={onToggleExpanded}
            onToggleTransfer={onToggleTransfer}
            onRetry={onRetry}
            onFinishLiveRecording={onFinishLiveRecording}
            onOpenFile={onOpenFile}
            onOpenFolder={onOpenFolder}
            compact={compact}
            className={cn("flex md:hidden", compact && "sm:col-start-2 sm:row-start-1 sm:self-center")}
          />
        )}

        {task.status === "failed" || task.status === "needs_attention" ? (
          <InlineRecovery
            task={task}
            expanded={expanded}
            compact={compact}
            onToggleExpanded={onToggleExpanded}
            onResolve={onResolveAttention}
          />
        ) : null}

        <AnimatePresence initial={false}>
          {expanded ? (
            <motion.div
              id={expandedId}
              className="col-span-full overflow-hidden"
              initial={reduceMotion ? false : { opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reduceMotion ? { opacity: 0 } : { opacity: 0, y: -4 }}
              transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            >
              <div className="grid gap-2 border-t border-border-divider pt-2 md:grid-cols-[minmax(0,1fr)_minmax(10rem,15rem)] md:items-center">
                <div className="min-w-0 space-y-1.5 text-xs text-text-secondary">
                  <DetailLine label={t("task.expanded.saveDir")} value={task.saveDir} />
                  <DetailLine
                    label={t("task.expanded.resume")}
                    value={
                      task.supportsParallel ? t("task.expanded.resumeSupported") : t("task.expanded.resumeUnavailable")
                    }
                  />
                  <div className="flex min-w-0 items-center gap-2">
                    <Activity className="h-3.5 w-3.5 shrink-0 text-accent-primary" aria-hidden />
                    <span className="min-w-0 truncate" title={diagnosticLabel}>
                      {diagnosticLabel}
                    </span>
                  </div>
                </div>
                <SpeedSparkline
                  samples={speedHistory}
                  currentSpeedBps={task.speedBps}
                  label={t("task.expanded.speedHistoryAria", {
                    name: task.fileName,
                  })}
                />
                <div className="md:col-span-2">
                  <TaskRecoveryActions task={task} onResolve={onResolveAttention} />
                </div>
              </div>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </TaskContextMenu>
  );
});

/// Local HH:MM for retry and completion timestamps. An unparseable (or empty)
/// value is returned as-is so `{{time}}` interpolations degrade to "" instead of
/// printing "Invalid Date".
function formatClockTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
  layout,
  compact,
}: {
  task: Task;
  isActive: boolean;
  layout: "inline" | "rail";
  compact: boolean;
}) {
  const { t } = useTranslation();
  const speed = formatSpeed(task.speedBps);
  const bytes = `${formatBytes(task.downloadedBytes)} / ${formatBytes(task.totalSize)}`;
  const percent = formatPercent(task.downloadedBytes, task.totalSize);
  const eta = formatEta(task.downloadedBytes, task.totalSize, task.speedBps);
  const connections = task.connectionCount > 0 ? t("task.connections", { count: task.connectionCount }) : null;
  // Connections ride along on the progress line rather than taking a rail row of
  // their own — a rail row costs ~20px, a third of a compact row's whole budget.
  const progress = [
    task.status === "completed" || eta === "—" ? percent : `${percent} · ${t("task.eta")} ${eta}`,
    connections,
  ]
    .filter(Boolean)
    .join(" · ");

  if (layout === "inline") {
    return (
      <div
        className={cn(
          "flex flex-wrap items-center gap-x-2 font-mono text-[11px] text-text-muted md:hidden",
          compact ? "gap-y-0.5" : "gap-y-1",
        )}
      >
        <span className={cn("text-text-primary", isActive && "text-xs font-semibold text-accent-primary")}>
          {speed}
        </span>
        {compact ? null : <span className={META_MUTED}>{bytes}</span>}
        <span className={META_MUTED} title={compact ? bytes : undefined}>
          {progress}
        </span>
      </div>
    );
  }

  if (compact) {
    // Two-line rail: bytes drop to the tooltip because percent already carries
    // the progress signal, and the freed line is what lets the row hit ~48px.
    return (
      <>
        <span
          data-slot="speed"
          className={cn(
            "col-start-1 row-start-1 min-w-0 truncate text-sm",
            isActive ? "font-semibold text-accent-primary" : "text-text-primary",
          )}
        >
          {speed}
        </span>
        <span data-slot="progress" title={bytes} className={cn("col-start-1 row-start-2 min-w-0 truncate", META_MUTED)}>
          {progress}
        </span>
      </>
    );
  }

  return (
    <>
      <span
        data-slot="speed"
        className={cn(
          "col-start-1 min-w-0 truncate text-sm",
          isActive ? "font-semibold text-accent-primary" : "text-text-primary",
        )}
      >
        {speed}
      </span>
      <span data-slot="bytes" className={cn("col-start-2 min-w-0 truncate", META_MUTED)}>
        {bytes}
      </span>
      <span data-slot="progress" className={cn("col-span-2 min-w-0 truncate", META_MUTED)}>
        {progress}
      </span>
    </>
  );
});

const DetailLine = memo(function DetailLine({ label, value }: { label: string; value: string }) {
  return (
    <p className="flex min-w-0 gap-2">
      <span className="shrink-0 text-text-muted">{label}</span>
      <span className="min-w-0 truncate text-text-secondary" title={value}>
        {value}
      </span>
    </p>
  );
});

function RowActions({
  task,
  expanded,
  expandedId,
  onToggleExpanded,
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  compact,
  className,
}: {
  task: Task;
  expanded: boolean;
  expandedId: string;
  onToggleExpanded: () => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  compact: boolean;
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
        // Compact still clears WCAG 2.5.8's 24px minimum while fitting the rail.
        compact &&
          "[&_[data-row-icon-button]]:h-9 [&_[data-row-icon-button]]:w-9 md:[&_[data-row-icon-button]]:h-7 md:[&_[data-row-icon-button]]:w-7",
        className,
      )}
      data-row-action
      data-no-drag
    >
      <ActionButton
        label={expanded ? t("actions.collapse") : t("actions.expand")}
        ariaLabel={t(expanded ? "actions.collapseFor" : "actions.expandFor", {
          name: task.fileName,
        })}
        expanded={expanded}
        controls={expandedId}
        onClick={(event) => {
          event.stopPropagation();
          onToggleExpanded();
        }}
      >
        <ChevronDown className={cn("h-4 w-4 transition-transform duration-ui", expanded && "rotate-180")} />
      </ActionButton>
      {transferMode !== "hidden" ? (
        <ActionButton
          label={transferMode === "resume" ? t("actions.resume") : t("actions.pause")}
          ariaLabel={t(transferMode === "resume" ? "actions.resumeFor" : "actions.pauseFor", {
            name: task.fileName,
          })}
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
          onClick={(event) => {
            event.stopPropagation();
            onOpenFile(task);
          }}
        >
          <File className="h-4 w-4" />
        </ActionButton>
      ) : null}
      <ActionButton
        label={t("actions.openFolder")}
        ariaLabel={t("actions.openFolderFor", { name: task.fileName })}
        onClick={(event) => {
          event.stopPropagation();
          onOpenFolder(task);
        }}
      >
        <FolderOpen className="h-4 w-4" />
      </ActionButton>
    </div>
  );
}

const ActionButton = memo(function ActionButton({
  label,
  ariaLabel,
  disabled,
  expanded,
  controls,
  onClick,
  children,
}: {
  label: string;
  ariaLabel?: string;
  disabled?: boolean;
  expanded?: boolean;
  controls?: string;
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
          aria-expanded={expanded}
          aria-controls={controls}
          disabled={disabled}
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
  expanded,
  compact,
  onToggleExpanded,
  onResolve,
}: {
  task: Task;
  expanded: boolean;
  compact: boolean;
  onToggleExpanded: () => void;
  onResolve: (task: Task, action: RecoveryAction) => void;
}) {
  const { t } = useTranslation();

  if (!task.errorMessage) return null;

  const recoveryActions = recoveryActionsForTask(task);

  if (recoveryActions.length === 0) return null;

  const primaryAction = recoveryActions[0];
  const hasMoreActions = recoveryActions.length > 1;
  const message = localizedErrorMessage(task.errorMessage, t);
  // Compact clamps the message to one line and shrinks the buttons at `md`+ only:
  // below `md` they are touch targets and must keep the 32px height.
  const buttonClass = compact ? "px-2 text-xs md:h-7 md:min-h-7" : "px-2 text-xs";

  return (
    // Alert container: a real callout box instead of loose inline elements.
    // Tinted bg + danger border + padding give the error its own visual unit,
    // so a failed row's recovery path reads as an alert, not as row text.
    <div
      className={cn(
        "col-span-full flex flex-wrap items-center rounded-md border border-border-danger-subtle bg-status-danger/[0.06] px-2.5",
        compact ? "mt-0.5 gap-1.5 py-1" : "mt-1 gap-2 py-2",
      )}
      data-row-action
      data-no-drag
    >
      <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-status-danger" aria-hidden />
      <span
        className={cn("min-w-0 text-xs leading-snug text-status-danger", compact ? "line-clamp-1" : "line-clamp-2")}
        title={message}
      >
        {message}
      </span>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button
          size="sm"
          className={buttonClass}
          onClick={(event) => {
            event.stopPropagation();
            onResolve(task, primaryAction);
          }}
        >
          {t(`recovery.${primaryAction}`)}
        </Button>
        {hasMoreActions ? (
          <Button
            variant="ghost"
            size="sm"
            className={buttonClass}
            onClick={(event) => {
              event.stopPropagation();
              if (!expanded) onToggleExpanded();
            }}
          >
            {t("actions.moreFixesCount", { count: recoveryActions.length - 1 })}
          </Button>
        ) : null}
      </div>
    </div>
  );
});
