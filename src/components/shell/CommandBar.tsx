import { ArrowUpDown, Command, Plus, Search, SlidersHorizontal } from "lucide-react";
import { type RefObject, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import type { Platform } from "@/lib/platform";
import { formatShortcut } from "@/lib/utils";
import { type TaskSortKey, useTaskUIStore } from "@/stores/task-store";

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
  const debouncedSearchInput = useDebouncedValue(searchInput, 300);
  // Debounce store writes here so TaskList/virtualizer do not rerender per keystroke.
  useEffect(() => {
    if (debouncedSearchInput !== search) setSearch(debouncedSearchInput);
  }, [debouncedSearchInput, search, setSearch]);
  useEffect(() => {
    setSearchInput((current) => (current === search ? current : search));
  }, [search]);
  const sortKey = useTaskUIStore((s) => s.sortKey);
  const sortDirection = useTaskUIStore((s) => s.sortDirection);
  const setSort = useTaskUIStore((s) => s.setSort);
  const nav = useTaskUIStore((s) => s.nav);
  const filters = useTaskUIStore((s) => s.filters);
  const toolPanelOpen = useTaskUIStore((s) => s.toolPanelOpen);
  const setToolPanelOpen = useTaskUIStore((s) => s.setToolPanelOpen);
  const showFilterButton =
    nav === "all" || nav === "downloading" || nav === "paused" || nav === "completed" || nav === "failed";
  const activeFilterCount =
    Number(filters.fileType !== "all") +
    Number(filters.source !== "all") +
    Number(filters.failure !== "all") +
    Number(filters.resume !== "all");

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
            {/* The label stays on at every width: below `md` the shell is a
                snapped desktop window with a compact icon rail, so the primary
                action is never reduced to a bare "+" glyph. */}
            <span className="inline">{t("commandBar.newDownload")}</span>
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

      {showFilterButton ? (
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
