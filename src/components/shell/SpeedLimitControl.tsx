//! Global speed-limit panel.
//!
//! Lives in the status bar rather than the command bar so the cap stays reachable
//! at every viewport tier: the command bar's copy was `md:flex`-only, which left a
//! narrow window with no way to change the limit short of opening Settings.

import { Check, Gauge, LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { applyGlobalSpeedLimit } from "@/lib/settings";
import {
  SPEED_LIMIT_PRESETS,
  SPEED_LIMIT_UNITS,
  speedLimitBytesFromInput,
  speedLimitInputFromBytes,
  speedLimitPresetLabel,
  speedLimitUnitLabel,
} from "@/lib/speed-limit";
import { cn, formatSpeed } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";

export function SpeedLimitControl({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation();
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const addToast = useToastStore((s) => s.addToast);
  const [saving, setSaving] = useState(false);
  const initialLimit = speedLimitInputFromBytes(
    settings?.globalSpeedLimitBps != null ? String(settings.globalSpeedLimitBps) : null,
  );
  const [customAmount, setCustomAmount] = useState(initialLimit.amount);
  const [customUnit, setCustomUnit] = useState(initialLimit.unit);
  const currentLimit = Number(settings?.globalSpeedLimitBps ?? 0);
  const label =
    currentLimit > 0
      ? t("statusBar.globalSpeedLimit", { speed: formatSpeed(currentLimit) })
      : t("statusBar.speedLimitOff");

  // Re-sync the custom fields when the limit changes from somewhere else —
  // a palette preset, the settings page, or the timed-window scheduler.
  useEffect(() => {
    const next = speedLimitInputFromBytes(
      settings?.globalSpeedLimitBps != null ? String(settings.globalSpeedLimitBps) : null,
    );
    setCustomAmount(next.amount);
    setCustomUnit(next.unit);
  }, [settings?.globalSpeedLimitBps]);

  async function apply(limit: number | null) {
    if (!settings) return;
    try {
      setSaving(true);
      setSettings(await applyGlobalSpeedLimit(settings, limit));
      onOpenChange(false);
    } catch (error) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  }

  function applyCustom() {
    const bytes = speedLimitBytesFromInput(customAmount, customUnit);
    // null = blank input (unlimited), undefined = invalid. Block both so Enter
    // cannot bypass the Apply button's disabled state.
    if (bytes == null) return;
    void apply(bytes);
  }

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          className="h-8 min-w-0 shrink-0 gap-1.5 px-1.5 text-[11px] font-normal text-text-muted hover:text-text-primary md:text-xs"
          aria-label={t("statusBar.changeSpeedLimit")}
          title={label}
          disabled={!settings || saving}
        >
          {saving ? (
            <LoaderCircle className="h-3.5 w-3.5 shrink-0 animate-spin" aria-hidden />
          ) : (
            <Gauge className={cn("h-3.5 w-3.5 shrink-0", currentLimit > 0 && "text-accent-primary")} aria-hidden />
          )}
          {/* An active cap is state the user must see at any width; "No speed
              limit" is the default and folds into the icon on a narrow bar. */}
          <span className={cn("truncate", currentLimit > 0 ? undefined : "hidden sm:inline")}>{label}</span>
        </Button>
      </PopoverTrigger>
      {/* The status bar is flush with the window's bottom edge, so the panel has
          to open upward or Radix clips it against the screen. */}
      <PopoverContent className="w-64" align="end" side="top">
        <fieldset className="m-0 space-y-0.5 border-0 p-0">
          <legend className="sr-only">{t("speedLimit.customBytes")}</legend>
          {SPEED_LIMIT_PRESETS.map((preset) => (
            <SpeedOption
              key={preset.id}
              label={speedLimitPresetLabel(preset.value)}
              active={preset.value === null ? currentLimit <= 0 : currentLimit === preset.value}
              onClick={() => void apply(preset.value)}
            />
          ))}
        </fieldset>
        <div className="mt-1.5 border-t border-border-subtle pt-1.5">
          <label
            htmlFor="status-bar-custom-speed-limit"
            className="block px-2 py-1 text-[11px] font-medium text-text-muted"
          >
            {t("speedLimit.custom")}
          </label>
          <div className="flex gap-1 px-1">
            <Input
              id="status-bar-custom-speed-limit"
              value={customAmount}
              onChange={(event) => setCustomAmount(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") applyCustom();
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
                    {speedLimitUnitLabel(unit.byteUnitKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={applyCustom}
              disabled={saving || !customAmount.trim()}
            >
              {t("speedLimit.apply")}
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

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
      {active ? <Check className="h-4 w-4 text-accent-primary" aria-hidden /> : null}
    </button>
  );
}
