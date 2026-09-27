import { ArrowUpDown, Check, Command, Keyboard, Plus, Search, SlidersHorizontal, X } from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { TranslationKey } from "@/i18n";
import type { Platform } from "@/lib/platform";
import { cn, formatShortcut } from "@/lib/utils";
import {
  type RowDensity,
  type TaskSortDirection,
  type TaskSortKey,
  useTaskDataStore,
  useTaskUIStore,
} from "@/stores/task-store";

interface CommandBarProps {
  platform: Platform;
  onOpenPalette: () => void;
  onNewDownload: () => void;
  inputRef?: RefObject<HTMLInputElement | null>;
  taskSurfaceActive?: boolean;
  /** True while onboarding is open, so the tip never stacks on top of it. */
  suppressFirstRunTip?: boolean;
  /** Rendered inside the titlebar (snapped or short windows): no row chrome of
   * its own, icon-only secondary controls, and the gaps stay draggable. */
  embedded?: boolean;
}

const SORT_OPTIONS = [
  { value: "updated_at:desc", labelKey: "taskList.sortUpdatedDesc" },
  { value: "created_at:desc", labelKey: "taskList.sortCreatedDesc" },
  { value: "file_size:desc", labelKey: "taskList.sortSizeDesc" },
  { value: "progress:desc", labelKey: "taskList.sortProgressDesc" },
  { value: "speed:desc", labelKey: "taskList.sortSpeedDesc" },
  { value: "status:asc", labelKey: "taskList.sortStatusAsc" },
] as const satisfies ReadonlyArray<{ value: `${TaskSortKey}:${TaskSortDirection}`; labelKey: TranslationKey }>;

const DENSITY_OPTIONS = [
  { value: "comfortable", labelKey: "taskList.densityComfortable" },
  { value: "compact", labelKey: "taskList.densityCompact" },
] as const satisfies ReadonlyArray<{ value: RowDensity; labelKey: TranslationKey }>;

/** Views whose list the filter panel narrows. */
const FILTERABLE_NAVS = new Set(["all", "downloading", "paused", "completed", "failed", "attention", "issues"]);

export function CommandBar({
  platform,
  onOpenPalette,
  onNewDownload,
  inputRef,
  taskSurfaceActive = true,
  suppressFirstRunTip = false,
  embedded = false,
}: CommandBarProps) {
  const { t } = useTranslation();
  const narrowShell = useShellLayout() === "narrow";
  const search = useTaskUIStore((s) => s.search);
  const setSearch = useTaskUIStore((s) => s.setSearch);
  const [searchInput, setSearchInput] = useState(search);
  const debouncedSearchInput = useDebouncedValue(searchInput, 300);
  // The last query this bar and the store agree on. Store writes are driven by
  // the *debounced input* only; the old effect also re-ran when the store
  // changed, saw the still-stale debounced text differ from a freshly cleared
  // store, and wrote the old query straight back — so neither the clear button
  // nor the empty state's "Clear search" could ever clear the search.
  const committedSearchRef = useRef(search);
  // Debounce store writes here so TaskList/virtualizer do not rerender per keystroke.
  useEffect(() => {
    if (debouncedSearchInput === committedSearchRef.current) return;
    committedSearchRef.current = debouncedSearchInput;
    setSearch(debouncedSearchInput);
  }, [debouncedSearchInput, setSearch]);
  // External writes (clear buttons elsewhere, the palette) win: adopt them as
  // the committed value so the pending debounced text cannot overwrite them.
  useEffect(() => {
    if (search === committedSearchRef.current) return;
    committedSearchRef.current = search;
    setSearchInput(search);
  }, [search]);
  const nav = useTaskUIStore((s) => s.nav);
  const filters = useTaskUIStore((s) => s.filters);
  const toolPanelOpen = useTaskUIStore((s) => s.toolPanelOpen);
  const setToolPanelOpen = useTaskUIStore((s) => s.setToolPanelOpen);
  const showFilterButton = FILTERABLE_NAVS.has(nav);
  const activeFilterCount =
    Number(filters.fileType !== "all") +
    Number(filters.source !== "all") +
    Number(filters.failure !== "all") +
    Number(filters.resume !== "all");

  // First-run tooltip: helps a new user find the button, so it shows only while
  // there are no tasks yet and onboarding is not already on screen. It used to
  // fire every launch, over onboarding, for users with hundreds of downloads.
  // P0b: previously suppressed entirely under prefers-reduced-motion, which removed
  // the hint for the audience that needs it most. Now we always show the tip; under
  // reduced-motion we extend the auto-dismiss to 10s so users have more time to read.
  const noTasksYet = useTaskDataStore((s) => !s.loading && (s.globalTaskStats ?? s.taskStats).all === 0);
  const tipEligible = noTasksYet && !suppressFirstRunTip;
  const [tipDone, setTipDone] = useState(false);
  useEffect(() => {
    if (!tipEligible || tipDone) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const delay = reducedMotion ? 10000 : 4000;
    const timer = window.setTimeout(() => setTipDone(true), delay);
    return () => window.clearTimeout(timer);
  }, [tipEligible, tipDone]);
  const firstRunTip = tipEligible && !tipDone;

  const clearSearch = () => {
    committedSearchRef.current = "";
    setSearchInput("");
    setSearch("");
  };

  return (
    <section
      className={cn(
        "flex min-w-0 items-center",
        embedded
          ? "h-full flex-1 gap-1.5 px-1"
          : "gap-1.5 border-b border-border-subtle bg-surface-base px-2 py-1.5 md:gap-2.5 md:px-3 md:py-2",
      )}
      aria-label={t("commandBar.toolbarAria")}
    >
      <Tooltip open={firstRunTip || undefined}>
        <TooltipTrigger asChild>
          <Button
            variant="default"
            className="h-11 shrink-0 gap-2 px-4 text-sm font-semibold md:h-8 md:px-3.5"
            aria-label={t("commandBar.newDownloadAria")}
            onClick={onNewDownload}
            data-no-drag
          >
            <Plus className="h-4 w-4" />
            {/* The label stays on at every width: below `md` the shell is a
                snapped desktop window with a compact icon rail, so the primary
                action is never reduced to a bare "+" glyph. */}
            <span className="inline">{t("commandBar.newDownload")}</span>
          </Button>
        </TooltipTrigger>
        <TooltipContent className={firstRunTip ? "max-w-56 text-balance" : undefined}>
          {firstRunTip ? (
            t("commandBar.newDownloadFirstRunTip", { shortcut: formatShortcut("mod+N", platform) })
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

      {taskSurfaceActive ? <ViewMenu compact={embedded} /> : null}

      {taskSurfaceActive && showFilterButton ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="relative h-11 w-11 shrink-0 md:h-8 md:w-8"
              aria-pressed={toolPanelOpen}
              aria-expanded={toolPanelOpen}
              aria-controls="task-list-tool-panel"
              aria-label={
                activeFilterCount > 0
                  ? t("taskList.toolPanelActive", { count: activeFilterCount })
                  : t(toolPanelOpen ? "taskList.hideToolPanel" : "taskList.showToolPanel")
              }
              onClick={() => setToolPanelOpen(!toolPanelOpen)}
              data-no-drag
            >
              <SlidersHorizontal className="h-4 w-4" />
              {activeFilterCount > 0 ? (
                <span className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-accent-primary" aria-hidden />
              ) : null}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("taskList.toolPanel")}</TooltipContent>
        </Tooltip>
      ) : null}

      {taskSurfaceActive ? (
        // Embedded in the titlebar the field stops growing past a readable
        // width, so the rest of the bar stays a window drag handle.
        <div className={cn("relative min-w-0 flex-1", embedded && "max-w-sm")} data-no-drag>
          <Search className="pointer-events-none absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
          <Input
            ref={inputRef}
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            onKeyDown={(event) => {
              // Esc clears a query before it blurs anything: the quickest way
              // back from an empty result.
              if (event.key === "Escape" && searchInput) {
                event.preventDefault();
                clearSearch();
              }
            }}
            placeholder={t(
              narrowShell || embedded ? "commandBar.searchPlaceholderShort" : "commandBar.searchPlaceholder",
            )}
            // The clear button's gutter is reserved only while there is text to
            // clear; an idle field keeps that width for the placeholder.
            className={cn("h-11 pl-8 md:h-8", searchInput ? "pr-8" : "pr-2")}
            aria-label={t("commandBar.searchAria")}
          />
          {searchInput ? (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="absolute right-0.5 top-1/2 h-9 w-9 -translate-y-1/2 text-text-muted hover:text-text-primary md:h-7 md:w-7"
              aria-label={t("settings.clearSearch")}
              onClick={clearSearch}
            >
              <X className="h-4 w-4" aria-hidden />
            </Button>
          ) : null}
        </div>
      ) : (
        <div className="min-w-0 flex-1" aria-hidden />
      )}

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="outline"
            size="icon"
            className={cn("h-11 w-11 shrink-0 md:h-8 md:w-8", !embedded && "md:hidden")}
            aria-label={t("commandBar.palette")}
            onClick={onOpenPalette}
            data-no-drag
          >
            {platform === "macos" ? <Command className="h-4 w-4" /> : <Keyboard className="h-4 w-4" />}
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          <span>{t("commandBar.palette")}</span>
          <kbd className="ml-1.5 rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-secondary">
            {formatShortcut("mod+K", platform)}
          </kbd>
        </TooltipContent>
      </Tooltip>

      {embedded ? null : (
        <Button
          variant="outline"
          size="sm"
          className="hidden shrink-0 gap-2 md:inline-flex"
          onClick={onOpenPalette}
          data-no-drag
        >
          {t("commandBar.palette")}
          <kbd className="ml-1 rounded border border-border-subtle bg-surface-root px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-secondary">
            {formatShortcut("mod+K", platform)}
          </kbd>
        </Button>
      )}
    </section>
  );
}

/**
 * Sort order and row height in one "View" menu. Row height used to hide inside
 * the Filters panel as "Row height", where nobody looks for a layout setting;
 * both are about how the list is shown, neither narrows it. From `md` up this
 * is the only copy; narrower windows keep both in the tool panel instead.
 */
function ViewMenu({ compact }: { compact: boolean }) {
  const { t } = useTranslation();
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const setSort = useTaskUIStore((s) => s.setSort);
  const rowDensity = useTaskUIStore((s) => s.rowDensity);
  const setRowDensity = useTaskUIStore((s) => s.setRowDensity);
  const sortValue = `${sortKey}:${sortDirection}`;
  const currentSort = SORT_OPTIONS.find((option) => option.value === sortValue);
  const sortLabel = currentSort ? t(currentSort.labelKey) : t("taskList.sort");
  const triggerLabel = t("commandBar.viewMenuAria", {
    sort: sortLabel,
    density: t(`taskList.density${rowDensity === "compact" ? "Compact" : "Comfortable"}`),
  });

  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              className={cn(
                "hidden h-8 shrink-0 gap-1.5 px-2 text-xs font-medium text-text-muted md:inline-flex",
                compact && "w-8 px-0",
              )}
              aria-label={triggerLabel}
              data-no-drag
            >
              <ArrowUpDown className="h-3.5 w-3.5 shrink-0" aria-hidden />
              {compact ? null : <span className="truncate">{sortLabel}</span>}
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{t("commandBar.viewMenu")}</TooltipContent>
      </Tooltip>
      <PopoverContent align="start" className="w-52 p-1.5">
        <ViewRadioGroup
          name="task-sort"
          legend={t("taskList.sort")}
          value={sortValue}
          options={SORT_OPTIONS.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
          onChange={(value) => {
            const [key, direction] = value.split(":") as [TaskSortKey, TaskSortDirection];
            setSort(key, direction);
          }}
        />
        <div className="my-1.5 h-px bg-border-divider" aria-hidden />
        <ViewRadioGroup
          name="task-density"
          legend={t("taskList.rowDensity")}
          value={rowDensity}
          options={DENSITY_OPTIONS.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
          onChange={(value) => setRowDensity(value === "compact" ? "compact" : "comfortable")}
        />
      </PopoverContent>
    </Popover>
  );
}

/** Native radios, so arrow keys move within each group without custom roving. */
function ViewRadioGroup({
  name,
  legend,
  value,
  options,
  onChange,
}: {
  name: string;
  legend: string;
  value: string;
  options: ReadonlyArray<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <fieldset className="m-0 min-w-0 border-0 p-0">
      <legend className="px-2 pb-1 pt-0.5 text-xs font-medium text-text-muted">{legend}</legend>
      {options.map((option) => {
        const checked = option.value === value;
        return (
          <label
            key={option.value}
            className={cn(
              "flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 text-sm",
              "transition-colors duration-[var(--motion-ui)] ease-out",
              "has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-accent-primary",
              checked ? "text-text-primary" : "text-text-secondary hover:bg-surface-raised hover:text-text-primary",
            )}
          >
            <input
              type="radio"
              name={name}
              value={option.value}
              checked={checked}
              onChange={() => onChange(option.value)}
              className="sr-only"
            />
            <Check className={cn("h-3.5 w-3.5 shrink-0 text-accent-primary", !checked && "invisible")} aria-hidden />
            {option.label}
          </label>
        );
      })}
    </fieldset>
  );
}
