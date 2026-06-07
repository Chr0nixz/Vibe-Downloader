import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, FolderOpen, LoaderCircle, Puzzle, RotateCcw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { AppSettings, BrowserIntegrationStatus } from "@/generated/bindings";
import { setLocale, type Locale } from "@/i18n";
import {
  getBrowserIntegrationStatus,
  getSettings,
  installBrowserIntegration,
  onBrowserIntegrationChanged,
  openDirectoryPicker,
  uninstallBrowserIntegration,
  updateSettings,
} from "@/lib/tauri";
import { createLogger } from "@/lib/logger";
import { cn } from "@/lib/utils";

const log = createLogger("settings");
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";

const AUTO_SAVE_DELAY_MS = 650;

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const settings = useSettingsStore((s) => s.settings);
  const loading = useSettingsStore((s) => s.loading);
  const error = useSettingsStore((s) => s.error);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const setLoading = useSettingsStore((s) => s.setLoading);
  const setError = useSettingsStore((s) => s.setError);
  const addToast = useToastStore((s) => s.addToast);
  const [defaultSaveDir, setDefaultSaveDir] = useState("");
  const [maxActiveTasks, setMaxActiveTasks] = useState(2);
  const [globalSpeedLimitBps, setGlobalSpeedLimitBps] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [browserStatus, setBrowserStatus] = useState<BrowserIntegrationStatus | null>(null);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserAction, setBrowserAction] = useState<string | null>(null);
  const autoSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveVersion = useRef(0);
  const currentLocale = (i18n.language === "zh-CN" ? "zh-CN" : "en") as Locale;
  const controlsDisabled = loading;

  useEffect(() => {
    if (!settings) return;
    setDefaultSaveDir(settings.defaultSaveDir);
    setMaxActiveTasks(settings.maxActiveTasks);
    setGlobalSpeedLimitBps(settings.globalSpeedLimitBps ?? "");
    setSaveState("saved");
  }, [settings]);

  useEffect(() => {
    if (!settings || loading) return;
    if (
      defaultSaveDir === settings.defaultSaveDir &&
      maxActiveTasks === settings.maxActiveTasks &&
      globalSpeedLimitBps === (settings.globalSpeedLimitBps ?? "")
    ) {
      return;
    }

    setSaveState("idle");
    if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    autoSaveTimer.current = setTimeout(() => {
      void saveSettings({
        defaultSaveDir,
        maxActiveTasks,
        globalSpeedLimitBps: globalSpeedLimitBps.trim(),
      });
    }, AUTO_SAVE_DELAY_MS);

    return () => {
      if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    };
  }, [defaultSaveDir, globalSpeedLimitBps, loading, maxActiveTasks, settings]);

  useEffect(() => {
    return () => {
      if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    };
  }, []);

  async function refreshSettings() {
    setLoading(true);
    setError(null);
    try {
      setSettings(await getSettings());
    } catch (err) {
      log.error("settings refresh failed", err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  async function saveSettings(nextSettings: AppSettings) {
    const version = ++saveVersion.current;
    setSaving(true);
    setSaveState("saving");
    setError(null);
    try {
      const next = await updateSettings({
        maxActiveTasks: nextSettings.maxActiveTasks,
        defaultSaveDir: nextSettings.defaultSaveDir,
        globalSpeedLimitBps: nextSettings.globalSpeedLimitBps,
      });
      if (version === saveVersion.current) {
        setSettings(next);
        setSaveState("saved");
        addToast({
          tone: "success",
          title: t("toast.settingsSaved"),
        });
      }
    } catch (err) {
      if (version === saveVersion.current) {
        log.error("settings save failed", err);
        setError(String(err));
        setSaveState("idle");
      }
    } finally {
      if (version === saveVersion.current) setSaving(false);
    }
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setDefaultSaveDir(selected);
  }

  function resetDirectory() {
    setDefaultSaveDir("");
  }

  useEffect(() => {
    if (!settings && !loading) void refreshSettings();
  }, []);

  useEffect(() => {
    void refreshBrowserIntegration();
    let unlisten: (() => void) | undefined;
    void onBrowserIntegrationChanged(() => {
      void refreshBrowserIntegration();
    }).then((next) => {
      unlisten = next;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  async function refreshBrowserIntegration() {
    setBrowserLoading(true);
    try {
      setBrowserStatus(await getBrowserIntegrationStatus());
    } catch (err) {
      log.error("browser integration status refresh failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: String(err),
      });
    } finally {
      setBrowserLoading(false);
    }
  }

  async function setBrowserInstalled(browser: BrowserIntegrationStatus["browsers"][number]) {
    setBrowserAction(browser.browser);
    try {
      const next = browser.manifestInstalled
        ? await uninstallBrowserIntegration({ browsers: [browser.browser] })
        : await installBrowserIntegration({ browsers: [browser.browser] });
      setBrowserStatus(next);
    } catch (err) {
      log.error("browser integration action failed", browser.browser, err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: String(err),
      });
    } finally {
      setBrowserAction(null);
    }
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-border-subtle bg-surface-base/55 px-4 py-4 sm:px-5">
          <div className="min-w-0">
            <h2 className="text-base font-semibold text-text-primary">
              {t("settings.title")}
            </h2>
            <p className="mt-1 max-w-2xl text-sm leading-5 text-text-secondary">
              {t("settings.description")}
            </p>
          </div>
          <SaveStatus
            state={saveState}
            saving={saving}
            className="hidden sm:inline-flex"
          />
        </div>

        <ScrollArea className="min-h-0 flex-1">
          <div className="mx-auto flex w-full max-w-4xl flex-col px-3 py-4 sm:px-4 md:px-6 md:py-5">
            {error ? (
              <p
                className="mb-4 rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
                role="alert"
              >
                {error}
              </p>
            ) : null}

            <SettingsSection
              title={t("settings.downloads")}
              description={t("settings.downloadsDescription")}
            >
              <SettingsRow
                title={t("settings.defaultSaveDir")}
                htmlFor="default-save-dir"
                controlClassName="max-w-2xl"
              >
                <div className="flex min-w-0 flex-col gap-2 sm:flex-row">
                  <Input
                    id="default-save-dir"
                    value={defaultSaveDir}
                    onChange={(event) => setDefaultSaveDir(event.target.value)}
                    placeholder={t("settings.defaultSaveDirPlaceholder")}
                    disabled={controlsDisabled}
                    className="h-11 min-w-0 bg-surface-root md:h-8"
                  />
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      className="h-11 shrink-0 md:h-8"
                      onClick={chooseDirectory}
                      disabled={controlsDisabled || saving}
                    >
                      <FolderOpen className="h-4 w-4" />
                      {t("settings.chooseDirectory")}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      className="h-11 shrink-0 md:h-8"
                      onClick={resetDirectory}
                      disabled={controlsDisabled || saving}
                    >
                      <RotateCcw className="h-4 w-4" />
                      {t("settings.reset")}
                    </Button>
                  </div>
                </div>
              </SettingsRow>

              <SettingsRow
                title={t("settings.maxActiveTasks")}
                htmlFor="max-active-tasks"
              >
                <Input
                  id="max-active-tasks"
                  type="number"
                  min={1}
                  max={8}
                  step={1}
                  value={maxActiveTasks}
                  onChange={(event) => {
                    const next = event.target.valueAsNumber;
                    if (Number.isFinite(next)) {
                      setMaxActiveTasks(Math.min(8, Math.max(1, next)));
                    }
                  }}
                  disabled={controlsDisabled}
                  className="h-11 w-28 bg-surface-root text-center font-mono md:h-8"
                />
              </SettingsRow>

              <SettingsRow
                title={t("settings.globalSpeedLimit")}
                htmlFor="global-speed-limit"
              >
                <Input
                  id="global-speed-limit"
                  type="number"
                  min={0}
                  step={1024}
                  value={globalSpeedLimitBps}
                  onChange={(event) => {
                    const value = event.target.value.trim();
                    if (value === "") {
                      setGlobalSpeedLimitBps("");
                      return;
                    }
                    const next = Number(value);
                    if (Number.isFinite(next) && next >= 0) {
                      setGlobalSpeedLimitBps(String(Math.floor(next)));
                    }
                  }}
                  placeholder={t("settings.globalSpeedLimitPlaceholder")}
                  disabled={controlsDisabled}
                  className="h-11 w-40 bg-surface-root text-center font-mono md:h-8"
                />
              </SettingsRow>
            </SettingsSection>

            <SettingsSection title={t("settings.interface")}>
              <SettingsRow title={t("locale.label")} htmlFor="locale-select">
                <select
                  id="locale-select"
                  value={currentLocale}
                  onChange={(event) => setLocale(event.target.value as Locale)}
                  className="h-11 w-full max-w-xs rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
                  disabled={controlsDisabled}
                >
                  <option value="en">{t("locale.en")}</option>
                  <option value="zh-CN">{t("locale.zhCN")}</option>
                </select>
              </SettingsRow>
            </SettingsSection>

            <SettingsSection
              title={t("settings.browserIntegration")}
              description={t("settings.browserIntegrationDescription")}
            >
              <div className="grid">
                {browserStatus?.browsers.map((browser) => {
                  const disabled =
                    browserLoading ||
                    browserAction === browser.browser ||
                    !browser.supportedOnPlatform;
                  const statusLabel = !browser.supportedOnPlatform
                    ? t("settings.browserUnsupported")
                    : browser.manifestInstalled
                      ? t("settings.browserInstalled")
                      : browser.detected
                        ? t("settings.browserDetected")
                        : t("settings.browserNotDetected");
                  return (
                    <div
                      key={browser.browser}
                      className="grid gap-3 border-t border-border-subtle/70 px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)_auto] md:items-center"
                    >
                      <div className="flex min-w-0 items-center gap-2 text-sm font-medium text-text-secondary">
                        <Puzzle className="h-4 w-4 shrink-0 text-text-muted" />
                        <span className="truncate">{browser.displayName}</span>
                      </div>
                      <div className="min-w-0">
                        <p className="text-sm text-text-primary">{statusLabel}</p>
                        <p className="mt-1 truncate text-xs text-text-muted">
                          {browser.manifestPath ?? t("settings.browserNoManifestPath")}
                        </p>
                        {browser.lastError ? (
                          <p className="mt-1 text-xs text-status-danger">{browser.lastError}</p>
                        ) : null}
                      </div>
                      <Button
                        type="button"
                        variant={browser.manifestInstalled ? "ghost" : "outline"}
                        className="h-11 justify-center md:h-9"
                        onClick={() => void setBrowserInstalled(browser)}
                        disabled={disabled}
                      >
                        {browser.manifestInstalled ? (
                          <Trash2 className="h-4 w-4" />
                        ) : (
                          <Check className="h-4 w-4" />
                        )}
                        {browser.manifestInstalled
                          ? t("settings.browserUninstall")
                          : t("settings.browserInstall")}
                      </Button>
                    </div>
                  );
                }) ?? (
                  <div className="flex items-center gap-2 px-4 py-4 text-sm text-text-muted">
                    <LoaderCircle className="h-4 w-4 animate-spin" />
                    {t("settings.browserLoading")}
                  </div>
                )}
              </div>
              {browserStatus ? (
                <div className="border-t border-border-subtle/70 px-4 py-3 text-xs leading-5 text-text-muted">
                  <p>
                    {t("settings.browserHostName")}{" "}
                    <span className="font-mono text-text-secondary">
                      {browserStatus.nativeHostName}
                    </span>
                  </p>
                  <p className="truncate">
                    {t("settings.browserExtensionPath")}{" "}
                    <span className="font-mono text-text-secondary">
                      {browserStatus.extensionCorePath ?? t("settings.browserBuildExtensions")}
                    </span>
                  </p>
                </div>
              ) : null}
            </SettingsSection>
          </div>
        </ScrollArea>

        <footer className="flex shrink-0 justify-end border-t border-border-subtle bg-surface-base/60 px-4 py-3 sm:hidden">
          <SaveStatus state={saveState} saving={saving} className="w-full justify-center" />
        </footer>
      </div>
    </div>
  );
}

function SaveStatus({
  state,
  saving,
  className,
}: {
  state: "idle" | "saving" | "saved";
  saving: boolean;
  className?: string;
}) {
  const { t } = useTranslation();
  const isSaving = saving || state === "saving";
  const label = isSaving
    ? t("settings.saving")
    : state === "saved"
      ? t("settings.saved")
      : t("settings.autoSave");

  return (
    <div
      className={cn(
        "inline-flex h-8 shrink-0 items-center gap-2 rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-secondary",
        className,
      )}
      aria-live="polite"
    >
      {isSaving ? (
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
      ) : state === "saved" ? (
        <Check className="h-4 w-4 text-status-success" />
      ) : null}
      {label}
    </div>
  );
}

function SettingsSection({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="border-t border-border-subtle py-5 first:border-t-0 first:pt-0">
      <div className="grid gap-1 pb-3">
        <h3 className="text-sm font-semibold text-text-primary">{title}</h3>
        {description ? (
          <p className="max-w-2xl text-sm leading-5 text-text-muted">
            {description}
          </p>
        ) : null}
      </div>
      <div className="overflow-hidden rounded-lg border border-border-subtle/75 bg-surface-base/70">
        {children}
      </div>
    </section>
  );
}

function SettingsRow({
  title,
  children,
  htmlFor,
  controlClassName,
}: {
  title: string;
  children: ReactNode;
  htmlFor?: string;
  controlClassName?: string;
}) {
  return (
    <div className="grid gap-3 border-t border-border-subtle/70 px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] md:items-center">
      <label className="text-sm font-medium text-text-secondary" htmlFor={htmlFor}>
        {title}
      </label>
      <div className={cn("min-w-0", controlClassName)}>{children}</div>
    </div>
  );
}
