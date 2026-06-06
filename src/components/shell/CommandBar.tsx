import {
  Command,
  Gauge,
  Pause,
  Play,
  Plus,
  Search,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
    <div className="flex min-w-0 items-center gap-1 border-b border-border-subtle bg-surface-base px-2 py-1.5 md:gap-2 md:px-3 md:py-2">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="default"
            size="icon"
            className="h-10 w-10 shrink-0 md:h-8 md:w-8"
            aria-label={t("commandBar.newDownloadAria")}
            onClick={onNewDownload}
          >
            <Plus className="h-4 w-4" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{t("commandBar.newDownload")}</TooltipContent>
      </Tooltip>

      <div className="hidden shrink-0 items-center gap-1 sm:flex md:gap-2">
        <ActionIcon
          label={t("commandBar.start")}
          icon={Play}
          onClick={onStart}
          disabled={!canStart}
        />
        <ActionIcon
          label={t("commandBar.pause")}
          icon={Pause}
          onClick={onPause}
          disabled={!canPause}
        />
        <ActionIcon
          label={t("commandBar.delete")}
          icon={Trash2}
          onClick={onDelete}
          disabled={!canDelete}
        />
        <ActionIcon
          label={t("commandBar.speedLimit")}
          icon={Gauge}
          className="hidden md:inline-flex"
        />
      </div>

      <div className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("commandBar.searchPlaceholder")}
          className="h-10 pl-8 md:h-8"
          aria-label={t("commandBar.searchAria")}
        />
      </div>

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="outline"
            size="icon"
            className="h-10 w-10 shrink-0 md:hidden"
            aria-label={t("commandBar.palette")}
            onClick={onOpenPalette}
          >
            <Command className="h-4 w-4" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{t("commandBar.palette")}</TooltipContent>
      </Tooltip>

      <Button
        variant="outline"
        size="sm"
        className="hidden shrink-0 md:inline-flex"
        onClick={onOpenPalette}
      >
        {t("commandBar.palette")}
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
  className,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
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
          className={className}
        >
          <Icon className="h-4 w-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
