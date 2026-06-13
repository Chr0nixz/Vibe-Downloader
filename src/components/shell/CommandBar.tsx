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
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import type { Platform } from "@/lib/platform";
import { applyGlobalSpeedLimit } from "@/lib/settings";
import { cn, formatShortcut, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import {
  useTaskStore,
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
  const search = useTaskStore((s) => s.search);
  const setSearch = useTaskStore((s) => s.setSearch);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const addToast = useToastStore((s) => s.addToast);
  const sortKey = useTaskStore((s) => s.sortKey);
  const sortDirection = useTaskStore((s) => s.sortDirection);
  const setSort = useTaskStore((s) => s.setSort);
  const [speedMenuOpen, setSpeedMenuOpen] = useState(false);
  const [customLimit, setCustomLimit] = useState("");
  const [savingSpeed, setSavingSpeed] = useState(false);
  const speedMenuRef = useRef<HTMLDivElement>(null);
  const speedTriggerRef = useRef<HTMLButtonElement>(null);
  const speedListRef = useRef<HTMLDivElement>(null);
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

  useEffect(() => {
    if (!speedMenuOpen) return;

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (
        target instanceof Node &&
        speedMenuRef.current &&
        !speedMenuRef.current.contains(target)
      ) {
        setSpeedMenuOpen(false);
      }
    };

    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [speedMenuOpen]);

  // Auto-focus the first menu item when the speed menu opens
  useEffect(() => {
    if (speedMenuOpen && speedListRef.current) {
      const firstItem = speedListRef.current.querySelector<HTMLElement>(
        '[role="menuitemradio"]',
      );
      firstItem?.focus();
    }
  }, [speedMenuOpen]);

  const handleSpeedMenuKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const menu = speedListRef.current;
      if (!menu) return;

      const items = Array.from(
        menu.querySelectorAll<HTMLElement>('[role="menuitemradio"]'),
      );
      if (items.length === 0) return;

      const currentIndex = items.indexOf(document.activeElement as HTMLElement);

      switch (event.key) {
        case "ArrowDown": {
          event.preventDefault();
          const next = currentIndex < items.length - 1 ? currentIndex + 1 : 0;
          items[next]?.focus();
          break;
        }
        case "ArrowUp": {
          event.preventDefault();
          const prev = currentIndex > 0 ? currentIndex - 1 : items.length - 1;
          items[prev]?.focus();
          break;
        }
        case "Home": {
          event.preventDefault();
          items[0]?.focus();
          break;
        }
        case "End": {
          event.preventDefault();
          items[items.length - 1]?.focus();
          break;
        }
        case "Escape": {
          event.preventDefault();
          setSpeedMenuOpen(false);
          speedTriggerRef.current?.focus();
          break;
        }
      }
    },
    [],
  );

  async function setSpeedLimit(limit: number | null) {
    try {
      setSavingSpeed(true);
      if (!settings) return;
      const nextSettings = await applyGlobalSpeedLimit(settings, limit);
      setSettings(nextSettings);
      setSpeedMenuOpen(false);
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
      <Tooltip>
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
        <TooltipContent>{t("commandBar.newDownload")}</TooltipContent>
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
        <div className="relative" ref={speedMenuRef}>
          <ActionIcon
            label={
              currentLimit > 0
                ? t("commandBar.speedLimitActive", {
                    speed: formatSpeed(currentLimit),
                  })
                : t("commandBar.speedLimit")
            }
            icon={savingSpeed ? LoaderCircle : Gauge}
            className={cn("hidden md:inline-flex", savingSpeed && "animate-spin")}
            onClick={() => setSpeedMenuOpen((open) => !open)}
            disabled={!settings || savingSpeed}
            buttonRef={speedTriggerRef}
            ariaHasPopup="menu"
            ariaExpanded={speedMenuOpen}
          />
          {speedMenuOpen ? (
            <div
              ref={speedListRef}
              className="absolute left-0 top-10 z-30 w-64 rounded-md border border-border-subtle bg-surface-overlay p-1.5 shadow-xl"
              role="menu"
              aria-label={t("commandBar.speedLimit")}
              onKeyDown={handleSpeedMenuKeyDown}
            >
              <SpeedPreset
                label={t("speedLimit.unlimited")}
                active={currentLimit <= 0}
                onClick={() => void setSpeedLimit(null)}
              />
              {SPEED_LIMIT_PRESETS.map((preset) => (
                <SpeedPreset
                  key={preset.value}
                  label={preset.label}
                  active={currentLimit === preset.value}
                  onClick={() => void setSpeedLimit(preset.value)}
                />
              ))}
              <div className="mt-1 border-t border-border-subtle pt-1">
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
            </div>
          ) : null}
        </div>
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
          value={search}
          onChange={(e) => setSearch(e.target.value)}
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

function SpeedPreset({
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
      role="menuitemradio"
      aria-checked={active}
      tabIndex={-1}
      className={cn(
        "flex h-8 w-full items-center justify-between rounded px-2 text-left text-sm text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
        active && "bg-surface-raised text-text-primary",
      )}
      onClick={onClick}
    >
      <span>{label}</span>
      {active ? <Check className="h-4 w-4 text-accent-primary" /> : null}
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
  ariaHasPopup,
  ariaExpanded,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
  buttonRef?: React.Ref<HTMLButtonElement>;
  ariaHasPopup?: React.AriaAttributes["aria-haspopup"];
  ariaExpanded?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          ref={buttonRef}
          variant="ghost"
          size="icon"
          aria-label={label}
          aria-haspopup={ariaHasPopup}
          aria-expanded={ariaExpanded}
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
