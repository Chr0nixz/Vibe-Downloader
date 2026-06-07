import { memo, type MouseEventHandler, type ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useTranslation } from "react-i18next";
import {
  Activity,
  ChevronDown,
  File,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
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

interface TaskRowProps {
  task: Task;
  selected: boolean;
  onSelect: () => void;
  onNavigate: (direction: "next" | "prev") => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  expanded: boolean;
  speedHistory: SpeedSample[];
  onToggleExpanded: () => void;
}

function statusTone(status: Task["status"]): string {
  switch (status) {
    case "downloading":
    case "retrying":
      return "text-accent-primary";
    case "completed":
      return "text-status-success";
    case "failed":
    case "needs_attention":
      return "text-status-danger";
    case "paused":
    case "queued":
    case "waiting_network":
      return "text-text-muted";
    default:
      return "text-text-secondary";
  }
}

export const TaskRow = memo(function TaskRow({
  task,
  selected,
  expanded,
  speedHistory,
  onSelect,
  onNavigate,
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
  onToggleExpanded,
}: TaskRowProps) {
  const { t } = useTranslation();
  const reduceMotion = useReducedMotion();
  const progress =
    task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
  const isActive =
    task.status === "downloading" || task.status === "retrying";
  const speedTrend = describeSpeedTrend(speedHistory, task.speedBps, t);
  const diagnosticLabel = task.healthSummary || speedTrend.label;
  const expandedId = `task-${task.id}-expanded`;
  const progressLabel = t("task.progressAria", {
    name: task.fileName,
    percent: formatPercent(task.downloadedBytes, task.totalSize),
  });

  return (
    <motion.div
      id={`task-option-${task.id}`}
      role="listitem"
      aria-current={selected ? "true" : undefined}
      aria-labelledby={`task-${task.id}-name`}
      tabIndex={selected ? 0 : -1}
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
        "relative overflow-hidden rounded-lg border border-border-subtle/70 bg-surface-base/85 px-3 py-3.5 shadow-[0_1px_2px_oklch(0_0_0_/_0.05),0_1px_0_oklch(1_0_0_/_0.025)_inset] transition-[background-color,border-color,box-shadow,transform] duration-200 ease-out hover:-translate-y-px hover:border-text-muted/30 hover:bg-surface-raised/70 hover:shadow-[0_6px_18px_oklch(0_0_0_/_0.08),0_1px_0_oklch(1_0_0_/_0.035)_inset] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/70 sm:px-3.5 md:px-4",
        "grid gap-x-4 gap-y-3 md:grid-cols-[minmax(0,1fr)_auto] md:gap-y-2",
        selected &&
          "border-accent-primary/35 bg-surface-raised ring-1 ring-accent-primary/45 shadow-[0_8px_24px_oklch(0_0_0_/_0.10),0_1px_0_oklch(1_0_0_/_0.04)_inset]",
        task.status === "completed" && "border-status-success/30",
        (task.status === "failed" || task.status === "needs_attention") &&
          "border-status-danger/35",
      )}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
    >
      <div className="min-w-0 space-y-2">
        <div className="flex min-w-0 items-start justify-between gap-2 md:block">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <div
                id={`task-${task.id}-name`}
                className="truncate text-sm font-medium text-text-primary"
              >
                {task.fileName}
              </div>
              <motion.span
                key={task.status}
                initial={reduceMotion ? false : { opacity: 0, scale: 0.96 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
                className={cn("shrink-0 text-xs font-medium", statusTone(task.status))}
              >
                {t(`task.status.${task.status}`)}
              </motion.span>
            </div>
            <p className="truncate text-xs text-text-muted">{task.sourceHost}</p>
          </div>
        </div>

        <p
          className={cn(
            "truncate text-xs text-text-secondary",
            speedTrend.tone === "warning" && !task.healthSummary && "text-status-warning",
            speedTrend.tone === "stable" && !task.healthSummary && "text-accent-energy",
          )}
        >
          {diagnosticLabel}
        </p>

        <ProgressBar value={progress} label={progressLabel} active={isActive} smooth={!isActive} />

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs text-text-muted md:hidden">
          <span className="text-text-primary">{formatSpeed(task.speedBps)}</span>
          <span>
            {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
          </span>
          <span>
            {formatPercent(task.downloadedBytes, task.totalSize)} · {t("task.eta")}{" "}
            {formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
          </span>
          {task.connectionCount > 0 ? (
            <span>{t("task.connections", { count: task.connectionCount })}</span>
          ) : null}
        </div>
      </div>

      <div className="hidden min-w-36 flex-col items-end gap-1 text-right font-mono text-xs md:flex">
        <span className="text-sm text-text-primary">{formatSpeed(task.speedBps)}</span>
        <span className="text-text-muted">
          {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
        </span>
        <span className="text-text-muted">
          {formatPercent(task.downloadedBytes, task.totalSize)} · {t("task.eta")}{" "}
          {formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
        </span>
        {task.connectionCount > 0 ? (
          <span className="text-text-muted">
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
        onOpenFile={onOpenFile}
        onOpenFolder={onOpenFolder}
        className="flex md:hidden"
      />

      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            id={expandedId}
            className="col-span-full overflow-hidden"
            initial={reduceMotion ? false : { opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={reduceMotion ? { opacity: 0 } : { opacity: 0, height: 0 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
          >
            <div className="grid gap-3 border-t border-border-subtle/70 pt-3 md:grid-cols-[minmax(0,1fr)_minmax(10rem,15rem)] md:items-center">
              <div className="min-w-0 space-y-2 text-xs text-text-secondary">
                <DetailLine label={t("task.expanded.saveDir")} value={task.saveDir} />
                <DetailLine
                  label={t("task.expanded.resume")}
                  value={
                    task.supportsRange
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
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </motion.div>
  );
});

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
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  className?: string;
}) {
  const { t } = useTranslation();
  const showsStart =
    task.status === "paused" ||
    task.status === "failed" ||
    task.status === "waiting_network";
  const transferDisabled =
    task.status === "completed" || task.status === "needs_attention";

  return (
    <div
      className={cn("flex gap-1.5 [&_button]:h-11 [&_button]:w-11 md:[&_button]:h-8 md:[&_button]:w-8", className)}
      data-row-action
      data-no-drag
    >
      <ActionButton
        label={expanded ? t("actions.collapse") : t("actions.expand")}
        expanded={expanded}
        controls={expandedId}
        onClick={(event) => {
          event.stopPropagation();
          onToggleExpanded();
        }}
      >
        <ChevronDown
          className={cn("h-4 w-4 transition-transform duration-200", expanded && "rotate-180")}
        />
      </ActionButton>
      <ActionButton
        label={showsStart ? t("actions.resume") : t("actions.pause")}
        disabled={transferDisabled}
        onClick={(event) => {
          event.stopPropagation();
          onToggleTransfer(task);
        }}
      >
        {showsStart ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
      </ActionButton>
      {task.status === "failed" ? (
        <ActionButton
          label={t("actions.retry")}
          onClick={(event) => {
            event.stopPropagation();
            onRetry(task);
          }}
        >
          <RotateCcw className="h-4 w-4" />
        </ActionButton>
      ) : null}
      {task.status === "completed" ? (
        <ActionButton
          label={t("actions.openFile")}
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
  disabled,
  expanded,
  controls,
  onClick,
  children,
}: {
  label: string;
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
          aria-label={label}
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
