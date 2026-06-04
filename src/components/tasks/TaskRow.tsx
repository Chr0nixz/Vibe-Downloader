import { memo } from "react";
import {
  File,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
} from "lucide-react";

import { Button } from "@/components/ui/button";
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

function statusLabel(status: Task["status"]): string {
  switch (status) {
    case "downloading":
      return "Downloading";
    case "paused":
      return "Paused";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
    case "retrying":
      return "Retrying";
    case "waiting_network":
      return "Waiting for network";
    case "needs_attention":
      return "Needs attention";
    default:
      return "Queued";
  }
}

export const TaskRow = memo(function TaskRow({
  task,
  selected,
  onSelect,
  onToggleTransfer,
  onRetry,
  onOpenFile,
  onOpenFolder,
}: TaskRowProps) {
  const progress =
    task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0;
  const isActive =
    task.status === "downloading" || task.status === "retrying";

  return (
    <article
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
      className={cn(
        "grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 gap-y-2 border-b border-border-subtle px-4 py-3 transition-colors hover:bg-surface-raised/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary",
        selected && "bg-surface-raised/80",
      )}
    >
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <h3 className="truncate text-sm font-medium text-text-primary">
            {task.fileName}
          </h3>
          <span className={cn("text-xs", statusTone(task.status))}>
            {statusLabel(task.status)}
          </span>
        </div>
        <p className="truncate text-xs text-text-muted">{task.sourceHost}</p>
        {task.healthSummary ? (
          <p className="mt-1 truncate text-xs text-text-secondary">
            {task.healthSummary}
          </p>
        ) : null}
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-surface-root">
          <div
            className={cn(
              "h-full rounded-full transition-[width] duration-200 ease-out",
              isActive ? "bg-accent-primary" : "bg-border-subtle",
            )}
            style={{ width: `${Math.max(2, progress * 100)}%` }}
          />
        </div>
      </div>

      <div className="flex flex-col items-end gap-1 text-right font-mono text-xs">
        <span className="text-text-primary">{formatSpeed(task.speedBps)}</span>
        <span className="text-text-muted">
          {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
        </span>
        <span className="text-text-muted">
          {formatPercent(task.downloadedBytes, task.totalSize)} · ETA{" "}
          {formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
        </span>
        {task.connectionCount > 0 ? (
          <span className="text-text-muted">{task.connectionCount} connections</span>
        ) : null}
        <div className="mt-1 flex gap-1" data-no-drag>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label={
              task.status === "paused" || task.status === "failed" ? "Resume" : "Pause"
            }
            disabled={task.status === "completed"}
            onClick={(e) => {
              e.stopPropagation();
              onToggleTransfer(task);
            }}
          >
            {task.status === "paused" || task.status === "failed" ? (
              <Play className="h-3.5 w-3.5" />
            ) : (
              <Pause className="h-3.5 w-3.5" />
            )}
          </Button>
          {task.status === "failed" || task.status === "needs_attention" ? (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              aria-label="Retry"
              onClick={(e) => {
                e.stopPropagation();
                onRetry(task);
              }}
            >
              <RotateCcw className="h-3.5 w-3.5" />
            </Button>
          ) : null}
          {task.status === "completed" ? (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              aria-label="Open file"
              onClick={(e) => {
                e.stopPropagation();
                onOpenFile(task);
              }}
            >
              <File className="h-3.5 w-3.5" />
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label="Open folder"
            onClick={(e) => {
              e.stopPropagation();
              onOpenFolder(task);
            }}
          >
            <FolderOpen className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </article>
  );
});
