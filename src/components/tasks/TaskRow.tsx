import { memo, useCallback, type MouseEventHandler, type ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import {
  Activity,
  AlertTriangle,
  ChevronDown,
  File,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
  Square,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { ProgressBar } from "@/components/ui/progress-bar";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  describeSpeedTrend,
  SpeedSparkline,
} from "@/components/tasks/SpeedSparkline";
import type { Task } from "@/types/task";
import {
  formatBytes,
  formatEta,
  formatPercent,
  formatSpeed,
} from "@/lib/utils";
import { cn } from "@/lib/utils";
import type { SpeedSample } from "@/stores/task-store";
import { useTaskStore } from "@/stores/task-store";
import { errorMessage, recoveryActionsForError } from "@/lib/errors";
import type { RecoveryAction } from "@/generated/bindings";
import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";

interface TaskRowProps {
  task: Task;
  selected: boolean;
  multiSelected: boolean;
  isFirstFocusable: boolean;
  reduceMotion: boolean;
  position: number;
  setSize: number;
  onSelectTask: (taskId: string) => void;
  onToggleSelected: (taskId: string, selected: boolean) => void;
  onNavigate: (direction: "next" | "prev") => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
}

const EMPTY_SPEED_HISTORY: SpeedSample[] = [];

function statusBadge(status: Task["status"]): string {
  switch (status) {
    case "downloading":
    case "retrying":
      return "bg-accent-primary/15 text-accent-primary";
    case "completed":
      return "bg-status-success/12 text-status-success";
    case "failed":
    case "needs_attention":
      return "bg-status-danger/12 text-status-danger";
    case "paused":
      return "bg-surface-raised text-text-muted";
    case "queued":
    case "waiting_network":
      return "bg-surface-raised text-text-secondary";
    default:
      return "bg-surface-raised text-text-secondary";
  }
}

export const TaskRow = memo(function TaskRow({
  task,
  selected,
  multiSelected,
  isFirstFocusable,
  reduceMotion,
  position,
  setSize,
  onSelectTask,
  onToggleSelected,
  onNavigate,
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onResolveAttention,
}: TaskRowProps) {
  const { t } = useTranslation();
  const expanded = useTaskStore((s) => s.expandedTaskIds.includes(task.id));
  const completionFlash = useTaskStore((s) => s.completionFlashIds.includes(task.id));
  const speedHistory = useTaskStore(
    (s) => s.speedHistoryByTaskId[task.id] ?? EMPTY_SPEED_HISTORY,
  );
  const toggleTaskExpanded = useTaskStore((s) => s.toggleTaskExpanded);
  const onSelect = useCallback(() => {
    onSelectTask(task.id);
  }, [onSelectTask, task.id]);
  const onToggleExpanded = useCallback(() => {
    toggleTaskExpanded(task.id);
  }, [toggleTaskExpanded, task.id]);
  const progress =
    task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
  const isActive =
    task.status === "downloading" || task.status === "retrying";
  const speedTrend = describeSpeedTrend(speedHistory, task.speedBps, t);
  const retryLaterLabel =
    task.retryAfterAt && task.status === "queued"
      ? t("task.retryAfter", { time: formatRetryTime(task.retryAfterAt) })
      : null;
  const diagnosticLabel = task.errorMessage
    ? errorMessage(task.errorMessage)
    : retryLaterLabel || task.healthSummary || speedTrend.label;
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
    <motion.div
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
        onSelect();
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
        "group relative overflow-hidden rounded-lg border border-transparent bg-surface-base/60 px-3 py-3.5 transition-[background-color,border-color,box-shadow,transform] duration-ui ease-out hover:-translate-y-px hover:bg-surface-raised/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/70 sm:px-3.5 md:px-4",
        "grid gap-x-4 gap-y-3 md:grid-cols-[minmax(0,1fr)_auto] md:gap-y-2",
        completionFlash && "completion-flash",
        selected &&
          "border-border-accent bg-accent-primary/[0.04]",
        multiSelected && !selected && "border-border-accent-subtle bg-accent-primary/[0.04]",
        task.status === "completed" && !selected && "border-border-success",
        (task.status === "failed" || task.status === "needs_attention") && !selected &&
          "border-border-danger-subtle",
      )}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
    >
      <div className="flex min-w-0 gap-3">
        <label
          className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded hover:bg-surface-raised"
          data-row-action
          onClick={(event) => event.stopPropagation()}
        >
          <span className="sr-only">
            {t("taskList.selectTask", { name: task.fileName })}
          </span>
          <input
            type="checkbox"
            checked={multiSelected}
            onChange={(event) => onToggleSelected(task.id, event.target.checked)}
            className="h-4 w-4 accent-accent-primary"
          />
        </label>
        <div className="min-w-0 flex-1 space-y-2">
        <div className="flex min-w-0 items-start justify-between gap-2 md:block">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
              <div
                id={nameId}
                className={cn(
                  "truncate text-[0.9rem] font-semibold leading-snug text-text-primary",
                  isActive && "text-[0.95rem]",
                )}
              >
                {task.fileName}
              </div>
              <motion.span
                id={statusId}
                key={task.status}
                initial={reduceMotion ? false : { opacity: 0, scale: 0.96 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
                className={cn(
                  "shrink-0 rounded-full px-2 py-0.5 text-[11px] font-semibold leading-none tracking-wide",
                  statusBadge(task.status),
                )}
              >
                {t(`task.status.${task.status}`)}
              </motion.span>
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
              : speedTrend.tone === "stable" && !task.healthSummary
                ? "font-medium text-accent-energy"
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
          size={isActive ? "lg" : "default"}
          className={completionFlash ? "completion-flash-progress" : undefined}
        />

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs text-text-muted md:hidden">
          <span className={cn("text-text-primary", isActive && "text-sm font-semibold text-accent-primary")}>{formatSpeed(task.speedBps)}</span>
          <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
            {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
          </span>
          <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
            {formatPercent(task.downloadedBytes, task.totalSize)} · {t("task.eta")}{" "}
            {formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
          </span>
          {task.connectionCount > 0 ? (
            <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
              {t("task.connections", { count: task.connectionCount })}
            </span>
          ) : null}
        </div>
        </div>
      </div>

      <div className="hidden min-w-36 flex-col items-end gap-1 text-right font-mono text-xs md:flex">
        <span className={cn(
          "text-sm text-text-primary",
          isActive && "text-base font-semibold text-accent-primary",
        )}>{formatSpeed(task.speedBps)}</span>
        <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
          {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
        </span>
        <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
          {formatPercent(task.downloadedBytes, task.totalSize)} · {t("task.eta")}{" "}
          {formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
        </span>
        {task.connectionCount > 0 ? (
          <span className="text-text-muted transition-colors duration-200 group-hover:text-text-secondary group-focus-within:text-text-secondary">
            {t("task.connections", { count: task.connectionCount })}
          </span>
        ) : null}
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
          className="mt-1"
        />
      </div>

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

      {(task.status === "failed" || task.status === "needs_attention") ? (
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
            <div className="grid gap-3 border-t border-border-divider pt-3 md:grid-cols-[minmax(0,1fr)_minmax(10rem,15rem)] md:items-center">
              <div className="min-w-0 space-y-2 text-xs text-text-secondary">
                <DetailLine label={t("task.expanded.saveDir")} value={task.saveDir} />
                <DetailLine
                  label={t("task.expanded.resume")}
                  value={
                    task.supportsParallel
                      ? t("task.expanded.resumeSupported")
                      : t("task.expanded.resumeUnavailable")
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
                <TaskRecoveryActions
                  task={task}
                  onResolve={onResolveAttention}
                  compact
                />
              </div>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.div>
  );
});

function formatRetryTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function DetailLine({ label, value }: { label: string; value: string }) {
  return (
    <p className="flex min-w-0 gap-2">
      <span className="shrink-0 text-text-muted">{label}</span>
      <span className="min-w-0 truncate text-text-secondary" title={value}>
        {value}
      </span>
    </p>
  );
}

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
  const showsStart =
    task.status === "paused" ||
    task.status === "failed" ||
    task.status === "waiting_network";
  const hideTransfer =
    task.status === "completed" || task.status === "needs_attention";
  const canFinishLiveRecording =
    task.protocol === "hls" && (task.status === "downloading" || task.status === "retrying");

  return (
    <div
      className={cn(
        "flex gap-1.5 [&_button]:h-11 [&_button]:w-11 md:[&_button]:h-8 md:[&_button]:w-8",
        "opacity-60 transition-opacity duration-ui group-hover:opacity-100 group-focus-within:opacity-100",
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
        <ChevronDown
          className={cn("h-4 w-4 transition-transform duration-ui", expanded && "rotate-180")}
        />
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
      {task.status === "failed" ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="sm"
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

function ActionButton({
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
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function InlineRecovery({
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
    <div
      className="col-span-full flex items-center gap-2"
      data-row-action
      data-no-drag
    >
      <AlertTriangle
        className="h-3.5 w-3.5 shrink-0 text-status-danger"
        aria-hidden
      />
      <Button
        size="sm"
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
          onClick={(event) => {
            event.stopPropagation();
            if (!expanded) onToggleExpanded();
          }}
        >
          {t("actions.moreFixes")}
        </Button>
      ) : null}
    </div>
  );
}
