import {
  ArrowUpDown,
  Check,
  Command,
  Gauge,
  LoaderCircle,
  Pause,
  Play,
  Plus,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import type { Platform } from "@/lib/platform";
import { applyGlobalSpeedLimit } from "@/lib/settings";
import { cn, formatShortcut, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import {
  useTaskUIStore,
  type TaskSortKey,
} from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
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
  const search = useTaskUIStore((s) => s.search);
  const setSearch = useTaskUIStore((s) => s.setSearch);
  const [searchInput, setSearchInput] = useState(search);
  const debouncedSearch = useDebouncedValue(searchInput, 200);
  useEffect(() => {
    setSearch(debouncedSearch);
  }, [debouncedSearch, setSearch]);
  useEffect(() => {
    if (search !== debouncedSearch) {
      setSearchInput(search);
    }
  }, [search, debouncedSearch]);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const addToast = useToastStore((s) => s.addToast);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const setSort = useTaskUIStore((s) => s.setSort);
  const [speedPopoverOpen, setSpeedPopoverOpen] = useState(false);
  const [customLimit, setCustomLimit] = useState("");
  const [savingSpeed, setSavingSpeed] = useState(false);
  const speedTriggerRef = useRef<HTMLButtonElement>(null);
  const canStart =
    !!selectedTask &&
    (selectedTask.status === "paused" ||
      selectedTask.status === "failed" ||
      selectedTask.status === "waiting_network");
  const canPause =
    !!selectedTask &&
    (selectedTask.status === "downloading" ||
      selectedTask.status === "retrying" ||
      selectedTask.status === "queued");
  const canDelete = !!selectedTask;
  const currentLimit = Number(settings?.globalSpeedLimitBps ?? 0);

  // First-run tooltip: auto-shows once per session to help new users discover the button
  const [firstRunTip, setFirstRunTip] = useState(true);
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setFirstRunTip(false);
      return;
    }
    const timer = window.setTimeout(() => setFirstRunTip(false), 4000);
    return () => window.clearTimeout(timer);
  }, []);

  async function setSpeedLimit(limit: number | null) {
    try {
      setSavingSpeed(true);
      if (!settings) return;
      const nextSettings = await applyGlobalSpeedLimit(settings, limit);
      setSettings(nextSettings);
      setSpeedPopoverOpen(false);
    } catch (error) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSavingSpeed(false);
    }
  }

  function applyCustomSpeed() {
    const parsed = Number(customLimit);
    if (!Number.isFinite(parsed) || parsed <= 0) return;
    void setSpeedLimit(Math.round(parsed));
  }

  return (
    <section
      className="flex min-w-0 items-center gap-1.5 border-b border-border-subtle bg-surface-base px-2 py-1.5 md:gap-2.5 md:px-3 md:py-2"
      aria-label={t("commandBar.toolbarAria")}
    >
      <Tooltip open={firstRunTip || undefined}>
        <TooltipTrigger asChild>
          <Button
            variant="default"
            className="h-11 shrink-0 gap-2 px-4 text-sm font-semibold md:h-9 md:px-3.5"
            aria-label={t("commandBar.newDownloadAria")}
            onClick={onNewDownload}
          >
            <Plus className="h-4 w-4" />
            <span className="hidden md:inline">{t("commandBar.newDownload")}</span>
          </Button>
        </TooltipTrigger>
        <TooltipContent className={firstRunTip ? "max-w-56 text-balance" : undefined}>
          {firstRunTip
            ? t("commandBar.newDownloadFirstRunTip")
            : t("commandBar.newDownload")}
        </TooltipContent>
      </Tooltip>

      <div className="hidden shrink-0 items-center gap-1 md:flex md:gap-2">
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
        <Popover open={speedPopoverOpen} onOpenChange={setSpeedPopoverOpen}>
          <PopoverTrigger asChild>
            <ActionIcon
              label={
                currentLimit > 0
                  ? t("commandBar.speedLimitActive", {
                      speed: formatSpeed(currentLimit),
                    })
                  : t("commandBar.speedLimit")
              }
              icon={savingSpeed ? LoaderCircle : Gauge}
              className={cn(
                "hidden md:inline-flex",
                savingSpeed && "animate-spin",
              )}
              disabled={!settings || savingSpeed}
              buttonRef={speedTriggerRef}
            />
          </PopoverTrigger>
          <PopoverContent className="w-64" align="start">
            <div className="space-y-0.5" role="radiogroup" aria-label={t("speedLimit.customBytes")}>
              <SpeedOption
                label={t("speedLimit.unlimited")}
                active={currentLimit <= 0}
                onClick={() => void setSpeedLimit(null)}
              />
              {SPEED_LIMIT_PRESETS.map((preset) => (
                <SpeedOption
                  key={preset.value}
                  label={preset.label}
                  active={currentLimit === preset.value}
                  onClick={() => void setSpeedLimit(preset.value)}
                />
              ))}
            </div>
            <div className="mt-1.5 border-t border-border-subtle pt-1.5">
              <label className="block px-2 py-1 text-[11px] font-medium text-text-muted">
                {t("speedLimit.customBytes")}
              </label>
              <div className="flex gap-1 px-1">
                <Input
                  value={customLimit}
                  onChange={(event) => setCustomLimit(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") applyCustomSpeed();
                  }}
                  inputMode="numeric"
                  placeholder="1048576"
                  className="h-8"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={applyCustomSpeed}
                  disabled={savingSpeed || !customLimit.trim()}
                >
                  {t("speedLimit.apply")}
                </Button>
              </div>
            </div>
          </PopoverContent>
        </Popover>
      </div>

      <div className="hidden shrink-0 items-center md:flex">
        <Select
          value={`${sortKey}:${sortDirection}`}
          onValueChange={(value) => {
            const [key, direction] = value.split(":") as [TaskSortKey, "asc" | "desc"];
            setSort(key, direction);
          }}
        >
          <SelectTrigger
            aria-label={t("taskList.sort")}
            title={t("taskList.sort")}
            className="h-8 w-auto gap-1.5 px-2 text-xs font-medium text-text-muted"
          >
            <ArrowUpDown className="h-3.5 w-3.5 shrink-0" aria-hidden />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="updated_at:desc">{t("taskList.sortUpdatedDesc")}</SelectItem>
            <SelectItem value="created_at:desc">{t("taskList.sortCreatedDesc")}</SelectItem>
            <SelectItem value="file_size:desc">{t("taskList.sortSizeDesc")}</SelectItem>
            <SelectItem value="progress:desc">{t("taskList.sortProgressDesc")}</SelectItem>
            <SelectItem value="speed:desc">{t("taskList.sortSpeedDesc")}</SelectItem>
            <SelectItem value="status:asc">{t("taskList.sortStatusAsc")}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
        <Input
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          placeholder={t("commandBar.searchPlaceholder")}
          className="h-11 pl-8 md:h-8"
          aria-label={t("commandBar.searchAria")}
        />
      </div>

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="outline"
            size="icon"
            className="h-11 w-11 shrink-0 md:hidden"
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
        className="hidden shrink-0 gap-2 md:inline-flex"
        onClick={onOpenPalette}
      >
        {t("commandBar.palette")}
        <kbd className="ml-1 rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-secondary">
          {formatShortcut("mod+K", platform)}
        </kbd>
      </Button>
    </section>
  );
}

const SPEED_LIMIT_PRESETS = [
  { label: "512 KB/s", value: 512 * 1024 },
  { label: "1 MB/s", value: 1024 * 1024 },
  { label: "5 MB/s", value: 5 * 1024 * 1024 },
  { label: "10 MB/s", value: 10 * 1024 * 1024 },
];

function SpeedOption({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active ? "true" : "false"}
      className={cn(
        "flex h-8 w-full items-center justify-between rounded-md px-2 text-left text-sm text-text-secondary",
        "transition-[background-color,color] duration-[var(--motion-ui)] ease-out",
        "hover:bg-surface-raised hover:text-text-primary",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        active && "bg-surface-raised text-text-primary",
      )}
      onClick={onClick}
    >
      <span>{label}</span>
      {active ? (
        <Check className="h-4 w-4 text-accent-primary" />
      ) : null}
    </button>
  );
}

function ActionIcon({
  label,
  icon: Icon,
  onClick,
  disabled,
  className,
  buttonRef,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
  buttonRef?: React.Ref<HTMLButtonElement>;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          ref={buttonRef}
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
