import {
  Gauge,
  Pause,
  Play,
  Plus,
  Search,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Platform } from "@/lib/platform";
import { formatShortcut } from "@/lib/utils";
import { useTaskStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

interface CommandBarProps {
  platform: Platform;
  onOpenPalette: () => void;
  selectedTask: Task | null;
  onNewDownload: () => void;
  onStart: () => void;
  onPause: () => void;
  onDelete: () => void;
}

export function CommandBar({
  platform,
  onOpenPalette,
  selectedTask,
  onNewDownload,
  onStart,
  onPause,
  onDelete,
}: CommandBarProps) {
  const search = useTaskStore((s) => s.search);
  const setSearch = useTaskStore((s) => s.setSearch);
  const canStart =
    !!selectedTask &&
    selectedTask.status !== "downloading" &&
    selectedTask.status !== "completed";
  const canPause =
    !!selectedTask &&
    (selectedTask.status === "downloading" || selectedTask.status === "retrying");
  const canDelete = !!selectedTask;

  return (
    <div className="flex items-center gap-2 border-b border-border-subtle bg-surface-base px-3 py-2">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="default"
            size="icon"
            aria-label="New download"
            onClick={onNewDownload}
          >
            <Plus className="h-4 w-4" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>New download</TooltipContent>
      </Tooltip>

      <ActionIcon label="Start" icon={Play} onClick={onStart} disabled={!canStart} />
      <ActionIcon label="Pause" icon={Pause} onClick={onPause} disabled={!canPause} />
      <ActionIcon label="Delete" icon={Trash2} onClick={onDelete} disabled={!canDelete} />
      <ActionIcon label="Speed limit" icon={Gauge} />

      <div className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search tasks"
          className="pl-8"
          aria-label="Search tasks"
        />
      </div>

      <Button variant="outline" size="sm" onClick={onOpenPalette}>
        Command palette
        <kbd className="ml-2 rounded border border-border-subtle px-1.5 py-0.5 font-mono text-[10px] text-text-muted">
          {formatShortcut("mod+K", platform)}
        </kbd>
      </Button>
    </div>
  );
}

function ActionIcon({
  label,
  icon: Icon,
  onClick,
  disabled,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onClick?: () => void;
  disabled?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={label}
          onClick={onClick}
          disabled={disabled}
        >
          <Icon className="h-4 w-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
