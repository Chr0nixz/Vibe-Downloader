import type { ReactElement, ReactNode } from "react";
import { cloneElement, isValidElement, useId } from "react";
import { useTranslation } from "react-i18next";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { BrowserCaptureSettings, BrowserForwardHeadersMode } from "@/generated/bindings";

import { SiteRulesEditor } from "./SiteRulesEditor";

interface BrowserCaptureControlsProps {
  settings: BrowserCaptureSettings;
  disabled?: boolean;
  onUpdate: (patch: Partial<BrowserCaptureSettings>) => void;
}

export function BrowserCaptureControls({ settings, disabled, onUpdate }: BrowserCaptureControlsProps) {
  const { t } = useTranslation();
  const experimental = settings.experimentalCaptureEnabled;

  return (
    <div className="grid border-b border-border-divider">
      <CaptureToggle
        title={t("settings.browserExperimentalCapture")}
        description={t("settings.browserExperimentalCaptureDescription")}
        checked={experimental}
        disabled={disabled}
        onChange={(experimentalCaptureEnabled) => onUpdate({ experimentalCaptureEnabled })}
      />
      {experimental ? (
        <>
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
          <CaptureField
            label={t("settings.browserForwardHeadersMode")}
            description={t("settings.browserForwardHeadersModeDescription")}
          >
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
          </CaptureField>
          <CaptureField label={t("settings.browserMinSize")} description={t("settings.browserMinSizeHint")}>
            <input
              type="number"
              min={0}
              step={1}
              value={bytesToMiB(settings.minSizeBytes)}
              disabled={disabled}
              onChange={(event) => {
                const mib = Number.parseFloat(event.target.value);
                onUpdate({
                  minSizeBytes: Number.isFinite(mib) && mib > 0 ? String(Math.round(mib * 1024 * 1024)) : "0",
                });
              }}
              className="w-full max-w-xs rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
            />
          </CaptureField>
          <CaptureField
            label={t("settings.browserFileExtensions")}
            description={t("settings.browserFileExtensionsHint")}
          >
            <textarea
              value={settings.fileExtensions.join(", ")}
              disabled={disabled}
              onChange={(event) =>
                onUpdate({
                  fileExtensions: event.target.value
                    .split(",")
                    .map((ext) => ext.trim())
                    .filter((ext) => ext.length > 0),
                })
              }
              rows={2}
              className="w-full max-w-xl rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
            />
          </CaptureField>
          <CaptureToggle
            title={t("settings.browserAllowIntranet")}
            description={t("settings.browserAllowIntranetHint")}
            checked={settings.allowIntranetHandoff}
            disabled={disabled}
            onChange={(allowIntranetHandoff) => onUpdate({ allowIntranetHandoff })}
          />
          {settings.allowIntranetHandoff ? (
            <p className="border-t border-border-divider px-4 py-2 text-xs leading-5 text-text-warning">
              {t("settings.browserAllowIntranetWarning")}
            </p>
          ) : null}
          <SiteRulesEditor
            rules={settings.siteRules}
            disabled={disabled}
            onUpdate={(siteRules) => onUpdate({ siteRules })}
          />
        </>
      ) : null}
    </div>
  );
}

function bytesToMiB(value: string): number {
  const bytes = Number.parseInt(value, 10);
  if (!Number.isFinite(bytes) || bytes <= 0) return 0;
  return bytes / (1024 * 1024);
}

function CaptureField({ label, description, children }: { label: string; description: string; children: ReactNode }) {
  const fieldId = useId();
  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] md:items-center">
      <label htmlFor={fieldId} className="text-sm font-medium text-text-secondary">
        {label}
      </label>
      <div className="grid gap-1">
        {isValidElement(children)
          ? cloneElement(children as ReactElement<{ id?: string; "aria-describedby"?: string }>, {
              id: fieldId,
              "aria-describedby": `${fieldId}-desc`,
            })
          : children}
        <p id={`${fieldId}-desc`} className="max-w-xl text-xs leading-5 text-text-muted">
          {description}
        </p>
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
      <Switch checked={checked} disabled={disabled} onCheckedChange={onChange} />
    </label>
  );
}
