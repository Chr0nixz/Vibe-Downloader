import { useTranslation } from "react-i18next";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { BrowserCaptureSettings, BrowserForwardHeadersMode } from "@/generated/bindings";

interface BrowserCaptureControlsProps {
  settings: BrowserCaptureSettings;
  disabled?: boolean;
  onUpdate: (patch: Partial<BrowserCaptureSettings>) => void;
}

export function BrowserCaptureControls({ settings, disabled, onUpdate }: BrowserCaptureControlsProps) {
  const { t } = useTranslation();

  return (
    <div className="grid border-b border-border-divider">
      <CaptureToggle
        title={t("settings.browserAutoIntercept")}
        description={t("settings.browserAutoInterceptDescription")}
        checked={settings.autoIntercept}
        disabled={disabled}
        onChange={(autoIntercept) => onUpdate({ autoIntercept })}
      />
      <CaptureToggle
        title={t("settings.browserForwardHeaders")}
        description={t("settings.browserForwardHeadersDescription")}
        checked={settings.forwardHeadersMode === "enabled"}
        disabled={disabled}
        onChange={(forwardHeaders) =>
          onUpdate({
            forwardHeadersMode: forwardHeaders ? "enabled" : "disabled",
          })
        }
      />
      <div className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] md:items-center">
        <label className="text-sm font-medium text-text-secondary">{t("settings.browserForwardHeadersMode")}</label>
        <div className="grid gap-1">
          <Select
            value={settings.forwardHeadersMode}
            onValueChange={(value) =>
              onUpdate({
                forwardHeadersMode: value as BrowserForwardHeadersMode,
              })
            }
            disabled={disabled}
          >
            <SelectTrigger aria-label={t("settings.browserForwardHeadersMode")} className="w-full max-w-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="ask">{t("settings.browserForwardHeadersAsk")}</SelectItem>
              <SelectItem value="enabled">{t("settings.browserForwardHeadersEnabled")}</SelectItem>
              <SelectItem value="disabled">{t("settings.browserForwardHeadersDisabled")}</SelectItem>
            </SelectContent>
          </Select>
          <p className="max-w-xl text-xs leading-5 text-text-muted">
            {t("settings.browserForwardHeadersModeDescription")}
          </p>
        </div>
      </div>
    </div>
  );
}

function CaptureToggle({
  title,
  description,
  checked,
  disabled,
  onChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <span className="min-w-0">
        <span className="block text-sm font-medium text-text-primary">{title}</span>
        <span className="mt-1 block text-xs leading-5 text-text-muted">{description}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="h-6 w-6 accent-accent-primary md:h-5 md:w-5"
      />
    </label>
  );
}
