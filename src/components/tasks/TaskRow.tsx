import { memo } from "react";
import { useTranslation } from "react-i18next";
import {
  File,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { ProgressBar } from "@/components/ui/progress-bar";
import type { Task } from "@/types/task";
import {
  formatBytes,
  formatEta,
  formatPercent,
  formatSpeed,
} from "@/lib/utils";
import { cn } from "@/lib/utils";

interface TaskRowProps {
  task: Task;
  selected: boolean;
  onSelect: () => void;
  onNavigate: (direction: "next" | "prev") => void;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
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
  onSelect,
  onNavigate,
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
}: TaskRowProps) {
  const { t } = useTranslation();
  const progress =
    task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
  const isActive =
    task.status === "downloading" || task.status === "retrying";
  const progressLabel = t("task.progressAria", {
    name: task.fileName,
    percent: formatPercent(task.downloadedBytes, task.totalSize),
  });

  return (
    <div
      id={`task-option-${task.id}`}
      role="option"
      aria-selected={selected}
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
        "border-b border-border-subtle px-3 py-3 transition-colors hover:bg-surface-raised/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary md:px-4",
        "grid gap-x-4 gap-y-3 md:grid-cols-[minmax(0,1fr)_auto] md:gap-y-2",
        selected && "bg-surface-raised/80",
      )}
    >
      <div className="min-w-0 space-y-2">
        <div className="flex min-w-0 items-start justify-between gap-2 md:block">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <h3
                id={`task-${task.id}-name`}
                className="truncate text-sm font-medium text-text-primary"
              >
                {task.fileName}
              </h3>
              <span className={cn("shrink-0 text-xs", statusTone(task.status))}>
                {t(`task.status.${task.status}`)}
              </span>
            </div>
            <p className="truncate text-xs text-text-muted">{task.sourceHost}</p>
          </div>
          <span className="shrink-0 font-mono text-sm text-text-primary md:hidden">
            {formatSpeed(task.speedBps)}
          </span>
        </div>

        {task.healthSummary ? (
          <p className="truncate text-xs text-text-secondary">{task.healthSummary}</p>
        ) : null}

        <ProgressBar value={progress} label={progressLabel} active={isActive} smooth={!isActive} />

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs text-text-muted md:hidden">
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

      <div className="hidden flex-col items-end gap-1 text-right font-mono text-xs md:flex">
        <span className="text-text-primary">{formatSpeed(task.speedBps)}</span>
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
          onToggleTransfer={onToggleTransfer}
          onRetry={onRetry}
          onOpenFile={onOpenFile}
          onOpenFolder={onOpenFolder}
          className="mt-1"
        />
      </div>

      <RowActions
        task={task}
        onToggleTransfer={onToggleTransfer}
        onRetry={onRetry}
        onOpenFile={onOpenFile}
        onOpenFolder={onOpenFolder}
        className="flex md:hidden"
      />
    </div>
  );
});

function RowActions({
  task,
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
  className,
}: {
  task: Task;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  className?: string;
}) {
  const { t } = useTranslation();

  return (
    <div
      className={cn("flex gap-1.5 [&_button]:h-10 [&_button]:w-10 md:[&_button]:h-8 md:[&_button]:w-8", className)}
      data-row-action
      data-no-drag
    >
      <Button
        variant="ghost"
        size="icon"
        aria-label={
          task.status === "paused" || task.status === "failed"
            ? t("actions.resume")
            : t("actions.pause")
        }
        disabled={task.status === "completed"}
        onClick={(event) => {
          event.stopPropagation();
          onToggleTransfer(task);
        }}
      >
        {task.status === "paused" || task.status === "failed" ? (
          <Play className="h-4 w-4" />
        ) : (
          <Pause className="h-4 w-4" />
        )}
      </Button>
      {task.status === "failed" || task.status === "needs_attention" ? (
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("actions.retry")}
          onClick={(event) => {
            event.stopPropagation();
            onRetry(task);
          }}
        >
          <RotateCcw className="h-4 w-4" />
        </Button>
      ) : null}
      {task.status === "completed" ? (
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("actions.openFile")}
          onClick={(event) => {
            event.stopPropagation();
            onOpenFile(task);
          }}
        >
          <File className="h-4 w-4" />
        </Button>
      ) : null}
      <Button
        variant="ghost"
        size="icon"
        aria-label={t("actions.openFolder")}
        onClick={(event) => {
          event.stopPropagation();
          onOpenFolder(task);
        }}
      >
        <FolderOpen className="h-4 w-4" />
      </Button>
    </div>
  );
}
