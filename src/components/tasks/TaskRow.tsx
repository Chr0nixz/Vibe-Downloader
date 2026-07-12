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
import { describeSpeedTrend, SpeedSparkline } from "@/components/tasks/SpeedSparkline";
import { type ReorderAction, TaskContextMenu } from "@/components/tasks/TaskContextMenu";
import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ProgressBar } from "@/components/ui/progress-bar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { RecoveryAction } from "@/generated/bindings";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useSystemFileIcon } from "@/hooks/use-system-file-icon";
import { localizedErrorMessage, localizedMessage, recoveryActionsForError } from "@/lib/errors";
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
}

const EMPTY_SPEED_HISTORY: SpeedSample[] = [];

function statusBadge(status: Task["status"]): string {
  switch (status) {
    case "downloading":
    case "retrying":
      return "bg-accent-primary/15 text-accent-primary";
    case "completed":
      return "bg-status-success/15 text-status-success";
    case "failed":
    case "needs_attention":
      return "bg-status-danger/15 text-status-danger";
    case "paused":
      return "bg-surface-raised text-text-muted ring-1 ring-border-subtle/60";
    case "queued":
    case "waiting_network":
      return "bg-surface-raised text-text-secondary ring-1 ring-border-subtle/60";
    default:
      return "bg-surface-raised text-text-secondary ring-1 ring-border-subtle/60";
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
}: TaskRowProps) {
  const { t } = useTranslation();
  const shellLayout = useShellLayout();
  const shellCompact = shellLayout === "narrow";
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
      ? t("task.retryAfter", { time: formatRetryTime(task.retryAfterAt) })
      : null;
  const healthSummary = localizedMessage(task.healthSummary, t);
  const diagnosticLabel = task.errorMessage
    ? localizedErrorMessage(task.errorMessage, t)
    : retryLaterLabel || healthSummary || speedTrend.label;
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
      <div
        id={`task-option-${task.id}`}
        role="option"
        aria-selected={selected || multiSelected}
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
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onSelect();
          }
        }}
        className={cn(
          // Row surface: bg-surface-base over list root gives a clearer row↔list delta now
          // that the surface scale is widened. Hover bumps to surface-raised (was /70 opacity,
          // now solid) and adds an inset lift so the row feels responsive without a stripe.
          "group relative overflow-hidden rounded-md border border-transparent bg-surface-base px-2.5 py-2 transition-[background-color,border-color,box-shadow] duration-ui ease-out hover:bg-surface-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary sm:px-3 md:px-3 md:py-1.5",
          "grid gap-x-3 gap-y-2 md:grid-cols-[minmax(0,1fr)_minmax(12rem,14rem)]",
          completionFlash && "completion-flash",
          // Selected: stronger accent fill (was 4%) + inset accent ring so the row anchors.
          selected &&
            "border-border-accent bg-accent-primary/10 shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--accent-primary)_20%,transparent)]",
          multiSelected && !selected && "border-border-accent-subtle bg-accent-primary/[0.06]",
          // Shift-select anchor: bump the tint so users can see the range origin.
          isShiftAnchor && (selected || multiSelected) && "bg-accent-primary/[0.14]",
          task.status === "completed" && !selected && "border-border-success/60",
          (task.status === "failed" || task.status === "needs_attention") && !selected && "border-border-danger-subtle",
        )}
      >
        <div className="flex min-w-0 gap-2.5">
          <label
            className="mt-0.5 flex h-11 w-11 shrink-0 items-center justify-center rounded hover:bg-surface-raised md:h-8 md:w-8"
            data-row-action
            onClick={(event) => event.stopPropagation()}
          >
            <span className="sr-only">{t("taskList.selectTask", { name: task.fileName })}</span>
            <Checkbox checked={multiSelected} onChange={(event) => onToggleSelected(task.id, event.target.checked)} />
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
                  <img src={systemIcon} alt="" className="h-11 w-11 object-contain md:h-8 md:w-8" draggable={false} />
                ) : (
                  <Icon className="h-10 w-10 md:h-7 md:w-7" />
                )}
              </span>
            );
          })()}
          <div className="min-w-0 flex-1 space-y-1.5 md:space-y-1">
            <div className="flex min-w-0 items-start justify-between gap-2 md:block">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <div id={nameId} className="truncate text-sm font-semibold leading-snug text-text-primary">
                    {task.fileName}
                  </div>
                  {(() => {
                    const badgeIcon = statusBadgeIcon(task.status);
                    return (
                      <motion.span
                        id={statusId}
                        initial={reduceMotion ? false : { opacity: 0, scale: 0.96 }}
                        animate={{ opacity: 1, scale: 1 }}
                        transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
                        className={cn(
                          "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold leading-none tracking-wide",
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
                <p id={hostId} className="truncate text-xs text-text-muted">
                  {task.sourceKey}
                </p>
              </div>
            </div>

            <p
              id={diagnosticId}
              className={cn(
                "truncate text-xs",
                speedTrend.tone === "warning" && !task.healthSummary
                  ? "font-medium text-status-warning"
                  : "text-text-secondary",
              )}
            >
              {diagnosticLabel}
            </p>

            <ProgressBar
              value={progress}
              label={progressLabel}
              active={isActive}
              smooth={!isActive}
              size="default"
              className={completionFlash ? "completion-flash-progress" : undefined}
            />

            <TaskMeta task={task} isActive={isActive} layout="inline" />
          </div>
        </div>

        {!shellCompact ? (
          <div className="hidden min-w-52 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-2 gap-y-1 text-right font-mono text-xs md:grid">
            <TaskMeta task={task} isActive={isActive} layout="rail" />
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
              className="col-span-2 justify-self-end"
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
            className="flex md:hidden"
          />
        )}

        {task.status === "failed" || task.status === "needs_attention" ? (
          <InlineRecovery
            task={task}
            expanded={expanded}
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
                    <span className="min-w-0 truncate">{diagnosticLabel}</span>
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

function formatRetryTime(value: string): string {
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
}: {
  task: Task;
  isActive: boolean;
  layout: "inline" | "rail";
}) {
  const { t } = useTranslation();
  const speed = formatSpeed(task.speedBps);
  const bytes = `${formatBytes(task.downloadedBytes)} / ${formatBytes(task.totalSize)}`;
  const progress = `${formatPercent(task.downloadedBytes, task.totalSize)} · ${t("task.eta")} ${formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}`;
  const connections = task.connectionCount > 0 ? t("task.connections", { count: task.connectionCount }) : null;

  if (layout === "inline") {
    return (
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[11px] text-text-muted md:hidden">
        <span className={cn("text-text-primary", isActive && "text-xs font-semibold text-accent-primary")}>
          {speed}
        </span>
        <span className={META_MUTED}>{bytes}</span>
        <span className={META_MUTED}>{progress}</span>
        {connections ? <span className={META_MUTED}>{connections}</span> : null}
      </div>
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
      {connections ? (
        <span data-slot="connections" className={cn("col-span-2", META_MUTED)}>
          {connections}
        </span>
      ) : null}
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
  className?: string;
}) {
  const { t } = useTranslation();
  const showsStart = task.status === "paused" || task.status === "failed" || task.status === "waiting_network";
  const hideTransfer = task.status === "completed" || task.status === "needs_attention";
  const canFinishLiveRecording =
    task.protocol === "hls" && (task.status === "downloading" || task.status === "retrying");
  // Hide RowActions retry button when InlineRecovery already shows "retry" as primary action,
  // to avoid duplicate retry entry points on failed task rows.
  const inlineRecoveryActions =
    task.recoveryActions && task.recoveryActions.length > 0
      ? task.recoveryActions
      : recoveryActionsForError(task.errorMessage ?? "");
  const hideRetryButton = task.status === "failed" && inlineRecoveryActions[0] === "retry";

  return (
    <div
      className={cn(
        "flex gap-1 [&_[data-row-icon-button]]:h-10 [&_[data-row-icon-button]]:w-10 md:[&_[data-row-icon-button]]:h-8 md:[&_[data-row-icon-button]]:w-8",
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
      {!hideTransfer ? (
        <ActionButton
          label={showsStart ? t("actions.resume") : t("actions.pause")}
          ariaLabel={t(showsStart ? "actions.resumeFor" : "actions.pauseFor", {
            name: task.fileName,
          })}
          onClick={(event) => {
            event.stopPropagation();
            onToggleTransfer(task);
          }}
        >
          {showsStart ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
        </ActionButton>
      ) : null}
      {task.status === "failed" && !hideRetryButton ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="sm"
              className="h-11 px-2 text-xs md:h-8 md:px-2"
              aria-label={t("actions.retryFor", { name: task.fileName })}
              onClick={(event) => {
                event.stopPropagation();
                onRetry(task);
              }}
            >
              <RotateCcw className="h-4 w-4" />
              {t("actions.retry")}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("actions.retryFor", { name: task.fileName })}</TooltipContent>
        </Tooltip>
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
  onToggleExpanded,
  onResolve,
}: {
  task: Task;
  expanded: boolean;
  onToggleExpanded: () => void;
  onResolve: (task: Task, action: RecoveryAction) => void;
}) {
  const { t } = useTranslation();

  if (!task.errorMessage) return null;

  const recoveryActions =
    task.recoveryActions && task.recoveryActions.length > 0
      ? task.recoveryActions
      : recoveryActionsForError(task.errorMessage);

  if (recoveryActions.length === 0) return null;

  const primaryAction = recoveryActions[0];
  const hasMoreActions = recoveryActions.length > 1;

  return (
    // Alert container: a real callout box instead of loose inline elements.
    // Tinted bg + danger border + padding give the error its own visual unit,
    // so a failed row's recovery path reads as an alert, not as row text.
    <div
      className="col-span-full mt-1 flex flex-wrap items-center gap-2 rounded-md border border-border-danger-subtle bg-status-danger/[0.06] px-2.5 py-2"
      data-row-action
      data-no-drag
    >
      <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-status-danger" aria-hidden />
      <span
        className="min-w-0 line-clamp-2 text-xs leading-snug text-status-danger"
        title={localizedErrorMessage(task.errorMessage, t)}
      >
        {localizedErrorMessage(task.errorMessage, t)}
      </span>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button
          size="sm"
          className="h-7 px-2 text-xs"
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
            className="h-7 px-2 text-xs"
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
