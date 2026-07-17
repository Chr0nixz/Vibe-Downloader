import { ArrowUpDown, Check, Command, Gauge, LoaderCircle, Plus, Search } from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Platform } from "@/lib/platform";
import { applyGlobalSpeedLimit } from "@/lib/settings";
import { SPEED_LIMIT_UNITS, speedLimitBytesFromInput, speedLimitInputFromBytes } from "@/lib/speed-limit";
import { cn, formatShortcut, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { type TaskSortKey, useTaskUIStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";

interface CommandBarProps {
  platform: Platform;
  onOpenPalette: () => void;
  onNewDownload: () => void;
  inputRef?: RefObject<HTMLInputElement | null>;
}

export function CommandBar({ platform, onOpenPalette, onNewDownload, inputRef }: CommandBarProps) {
  const { t } = useTranslation();
  const search = useTaskUIStore((s) => s.search);
  const setSearch = useTaskUIStore((s) => s.setSearch);
  const [searchInput, setSearchInput] = useState(search);
  // UX-2: Write to store immediately on each keystroke. TaskList's 300ms
  // useDebouncedValue is the single debounce layer — the previous serial
  // 200ms + 300ms = 500ms delay is eliminated.
  useEffect(() => {
    setSearch(searchInput);
  }, [searchInput, setSearch]);
  useEffect(() => {
    if (search !== searchInput) {
      setSearchInput(search);
    }
  }, [search, searchInput]);
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const addToast = useToastStore((s) => s.addToast);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const setSort = useTaskUIStore((s) => s.setSort);
  const [speedPopoverOpen, setSpeedPopoverOpen] = useState(false);
  const initialLimit = speedLimitInputFromBytes(
    settings?.globalSpeedLimitBps != null ? String(settings.globalSpeedLimitBps) : null,
  );
  const [customAmount, setCustomAmount] = useState(initialLimit.amount);
  const [customUnit, setCustomUnit] = useState(initialLimit.unit);
  const [savingSpeed, setSavingSpeed] = useState(false);
  const speedTriggerRef = useRef<HTMLButtonElement>(null);
  const currentLimit = Number(settings?.globalSpeedLimitBps ?? 0);
  const speedLabel =
    currentLimit > 0
      ? t("commandBar.speedLimitActive", { speed: formatSpeed(currentLimit) })
      : t("commandBar.speedLimit");

  // First-run tooltip: auto-shows once per session to help new users discover the button.
  // P0b: previously suppressed entirely under prefers-reduced-motion, which removed
  // the hint for the audience that needs it most. Now we always show the tip; under
  // reduced-motion we extend the auto-dismiss to 10s so users have more time to read.
  const [firstRunTip, setFirstRunTip] = useState(true);
  useEffect(() => {
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const delay = reducedMotion ? 10000 : 4000;
    const timer = window.setTimeout(() => setFirstRunTip(false), delay);
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
    const bytes = speedLimitBytesFromInput(customAmount, customUnit);
    // null = blank input (unlimited), undefined = invalid. Block both to match
    // the Apply button's disabled state and avoid Enter bypassing it.
    if (bytes == null) return;
    void setSpeedLimit(bytes);
  }

  // Keep the amount/unit fields in sync when the global limit changes elsewhere.
  useEffect(() => {
    const next = speedLimitInputFromBytes(
      settings?.globalSpeedLimitBps != null ? String(settings.globalSpeedLimitBps) : null,
    );
    setCustomAmount(next.amount);
    setCustomUnit(next.unit);
  }, [settings?.globalSpeedLimitBps]);

  return (
    <section
      className="flex min-w-0 items-center gap-1.5 border-b border-border-subtle bg-surface-base px-2 py-1.5 md:gap-2.5 md:px-3 md:py-2"
      aria-label={t("commandBar.toolbarAria")}
    >
      <Tooltip open={firstRunTip || undefined}>
        <TooltipTrigger asChild>
          <Button
            variant="default"
            className="h-11 shrink-0 gap-2 px-4 text-sm font-semibold md:h-8 md:px-3.5"
            aria-label={t("commandBar.newDownloadAria")}
            onClick={onNewDownload}
          >
            <Plus className="h-4 w-4" />
            <span className="hidden md:inline">{t("commandBar.newDownload")}</span>
          </Button>
        </TooltipTrigger>
        <TooltipContent className={firstRunTip ? "max-w-56 text-balance" : undefined}>
          {firstRunTip ? (
            t("commandBar.newDownloadFirstRunTip")
          ) : (
            <>
              <span>{t("commandBar.newDownload")}</span>
              <kbd className="ml-1.5 rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-secondary">
                {formatShortcut("mod+N", platform)}
              </kbd>
            </>
          )}
        </TooltipContent>
      </Tooltip>

      <div className="hidden shrink-0 items-center gap-1 md:flex md:gap-2">
        <Popover open={speedPopoverOpen} onOpenChange={setSpeedPopoverOpen}>
          <Tooltip>
            <TooltipTrigger asChild>
              <PopoverTrigger asChild>
                <Button
                  ref={speedTriggerRef}
                  variant="ghost"
                  size="icon"
                  aria-label={speedLabel}
                  disabled={!settings || savingSpeed}
                  className={cn("hidden md:inline-flex", savingSpeed && "animate-spin")}
                >
                  {savingSpeed ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Gauge className="h-4 w-4" />}
                </Button>
              </PopoverTrigger>
            </TooltipTrigger>
            <TooltipContent>
              <span>{speedLabel}</span>
            </TooltipContent>
          </Tooltip>
          <PopoverContent className="w-64" align="start">
            <fieldset className="m-0 space-y-0.5 border-0 p-0">
              <legend className="sr-only">{t("speedLimit.customBytes")}</legend>
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
            </fieldset>
            <div className="mt-1.5 border-t border-border-subtle pt-1.5">
              <label
                htmlFor="command-bar-custom-speed-limit"
                className="block px-2 py-1 text-[11px] font-medium text-text-muted"
              >
                {t("speedLimit.custom")}
              </label>
              <div className="flex gap-1 px-1">
                <Input
                  id="command-bar-custom-speed-limit"
                  value={customAmount}
                  onChange={(event) => setCustomAmount(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") applyCustomSpeed();
                  }}
                  inputMode="decimal"
                  placeholder={t("settings.globalSpeedLimitPlaceholder")}
                  aria-label={t("speedLimit.custom")}
                  className="h-8"
                />
                <Select value={customUnit} onValueChange={setCustomUnit}>
                  <SelectTrigger aria-label={t("settings.speedUnit")} className="h-8 w-auto min-w-[5rem] px-2 text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {SPEED_LIMIT_UNITS.map((unit) => (
                      <SelectItem key={unit.value} value={unit.value}>
                        {unit.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={applyCustomSpeed}
                  disabled={savingSpeed || !customAmount.trim()}
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
          ref={inputRef}
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
        <TooltipContent>
          <span>{t("commandBar.palette")}</span>
          <kbd className="ml-1.5 rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-secondary">
            {formatShortcut("mod+K", platform)}
          </kbd>
        </TooltipContent>
      </Tooltip>

      <Button variant="outline" size="sm" className="hidden shrink-0 gap-2 md:inline-flex" onClick={onOpenPalette}>
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

function SpeedOption({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-pressed={active}
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
      {active ? <Check className="h-4 w-4 text-accent-primary" /> : null}
    </button>
  );
}
