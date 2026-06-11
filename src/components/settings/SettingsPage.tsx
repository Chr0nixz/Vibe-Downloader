import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Archive,
  Check,
  Clipboard,
  FolderOpen,
  Info,
  LoaderCircle,
  Puzzle,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { useTheme } from "next-themes";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type {
  AppAccentColor,
  AppFontFamily,
  AppProxyMode,
  AppSettings,
  BrowserCaptureSettings,
  BrowserExtensionExportResult,
  BrowserForwardHeadersMode,
  BrowserIntegrationStatus,
} from "@/generated/bindings";
import { setLocale, type Locale } from "@/i18n";
import {
  exportBrowserExtensionPackages,
  getBrowserCaptureSettings,
  getBrowserIntegrationStatus,
  getSettings,
  installBrowserIntegration,
  isTauriRuntime,
  onBrowserIntegrationChanged,
  openDirectoryPicker,
  uninstallBrowserIntegration,
  updateBrowserCaptureSettings,
  updateSettings,
} from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { cn } from "@/lib/utils";

const log = createLogger("settings");
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";

const AUTO_SAVE_DELAY_MS = 650;
const MAX_SEGMENT_COUNT = 8;
const MAX_CONNECTIONS_PER_HOST = 16;

const ACCENT_HUES: Record<AppAccentColor, number> = {
  blue: 235,
  purple: 290,
  teal: 190,
  green: 150,
  orange: 55,
  rose: 350,
  indigo: 265,
  amber: 80,
};

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const { theme, resolvedTheme, setTheme } = useTheme();
  const isDark = resolvedTheme === "dark";
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
  const [multiConnectionThresholdBytes, setMultiConnectionThresholdBytes] = useState("");
  const [segmentCount, setSegmentCount] = useState(4);
  const [maxConnectionsPerHost, setMaxConnectionsPerHost] = useState(8);
  const [systemNotifications, setSystemNotifications] = useState(true);
  const [closeToTray, setCloseToTray] = useState(false);
  const [startOnBoot, setStartOnBoot] = useState(false);
  const [floatingWindowEnabled, setFloatingWindowEnabled] = useState(false);
  const [fontFamily, setFontFamily] = useState<AppFontFamily>("source_han_sans_sc");
  const [accentColor, setAccentColor] = useState<AppAccentColor>("blue");
  const [proxyMode, setProxyMode] = useState<AppProxyMode>("off");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyNoProxy, setProxyNoProxy] = useState("");
  const [proxyUsername, setProxyUsername] = useState("");
  const [proxyPassword, setProxyPassword] = useState("");
  const [proxyPasswordSaved, setProxyPasswordSaved] = useState(false);
  const [clearProxyPassword, setClearProxyPassword] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [browserStatus, setBrowserStatus] = useState<BrowserIntegrationStatus | null>(null);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserAction, setBrowserAction] = useState<string | null>(null);
  const [browserExporting, setBrowserExporting] = useState(false);
  const [browserExportResult, setBrowserExportResult] =
    useState<BrowserExtensionExportResult | null>(null);
  const [browserCapture, setBrowserCapture] = useState<BrowserCaptureSettings | null>(null);
  const [browserCaptureSaving, setBrowserCaptureSaving] = useState(false);
  const autoSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveVersion = useRef(0);
  const currentLocale = (["zh-CN", "zh-TW", "ja", "ko", "ru", "es", "en"].includes(i18n.language)
    ? (i18n.language as Locale)
    : "en") as Locale;
  const controlsDisabled = loading;

  useEffect(() => {
    if (!settings) return;
    setDefaultSaveDir(settings.defaultSaveDir);
    setMaxActiveTasks(settings.maxActiveTasks);
    setGlobalSpeedLimitBps(settings.globalSpeedLimitBps ?? "");
    setMultiConnectionThresholdBytes(settings.multiConnectionThresholdBytes);
    setSegmentCount(settings.segmentCount);
    setMaxConnectionsPerHost(settings.maxConnectionsPerHost);
    setSystemNotifications(settings.systemNotifications);
    setCloseToTray(settings.closeToTray);
    setStartOnBoot(settings.startOnBoot);
    setFloatingWindowEnabled(settings.floatingWindowEnabled);
    setFontFamily(settings.fontFamily);
    setAccentColor(settings.accentColor);
    setProxyMode(settings.proxyMode);
    setProxyUrl(settings.proxyUrl);
    setProxyNoProxy(settings.proxyNoProxy);
    setProxyUsername(settings.proxyUsername);
    setProxyPassword("");
    setProxyPasswordSaved(settings.proxyPasswordSaved);
    setClearProxyPassword(false);
    setSaveState("saved");
  }, [settings]);

  useEffect(() => {
    if (!settings || loading) return;
    if (
      defaultSaveDir === settings.defaultSaveDir &&
      maxActiveTasks === settings.maxActiveTasks &&
      globalSpeedLimitBps === (settings.globalSpeedLimitBps ?? "") &&
      multiConnectionThresholdBytes === settings.multiConnectionThresholdBytes &&
      segmentCount === settings.segmentCount &&
      maxConnectionsPerHost === settings.maxConnectionsPerHost &&
      systemNotifications === settings.systemNotifications &&
      closeToTray === settings.closeToTray &&
      startOnBoot === settings.startOnBoot &&
      floatingWindowEnabled === settings.floatingWindowEnabled &&
      fontFamily === settings.fontFamily &&
      accentColor === settings.accentColor &&
      proxyMode === settings.proxyMode &&
      proxyUrl === settings.proxyUrl &&
      proxyNoProxy === settings.proxyNoProxy &&
      proxyUsername === settings.proxyUsername &&
      proxyPassword.trim() === "" &&
      !clearProxyPassword
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
        multiConnectionThresholdBytes: multiConnectionThresholdBytes.trim(),
        segmentCount,
        maxConnectionsPerHost,
        systemNotifications,
        closeToTray,
        startOnBoot,
        floatingWindowEnabled,
        fontFamily,
        accentColor,
        proxyMode,
        proxyUrl,
        proxyNoProxy,
        proxyUsername,
        proxyPasswordSaved,
      });
    }, AUTO_SAVE_DELAY_MS);

    return () => {
      if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    };
  }, [
    defaultSaveDir,
    globalSpeedLimitBps,
    loading,
    maxActiveTasks,
    maxConnectionsPerHost,
    multiConnectionThresholdBytes,
    segmentCount,
    settings,
    systemNotifications,
    closeToTray,
    startOnBoot,
    floatingWindowEnabled,
    fontFamily,
    accentColor,
    proxyMode,
    proxyUrl,
    proxyNoProxy,
    proxyUsername,
    proxyPassword,
    proxyPasswordSaved,
    clearProxyPassword,
  ]);

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
      setError(errorMessage(err));
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
        multiConnectionThresholdBytes: nextSettings.multiConnectionThresholdBytes,
        segmentCount: nextSettings.segmentCount,
        maxConnectionsPerHost: nextSettings.maxConnectionsPerHost,
        systemNotifications: nextSettings.systemNotifications,
        closeToTray: nextSettings.closeToTray,
        startOnBoot: nextSettings.startOnBoot,
        floatingWindowEnabled: nextSettings.floatingWindowEnabled,
        fontFamily: nextSettings.fontFamily,
        accentColor: nextSettings.accentColor,
        proxyMode: nextSettings.proxyMode,
        proxyUrl: nextSettings.proxyUrl,
        proxyNoProxy: nextSettings.proxyNoProxy,
        proxyUsername: nextSettings.proxyUsername,
        proxyPassword: proxyPassword.trim() || null,
        clearProxyPassword,
      });
      if (next.startOnBoot !== settings?.startOnBoot) {
        await syncAutostart(next.startOnBoot);
      }
      if (version === saveVersion.current) {
        setSettings(next);
        setProxyPassword("");
        setProxyPasswordSaved(next.proxyPasswordSaved);
        setClearProxyPassword(false);
        setSaveState("saved");
        addToast({
          tone: "success",
          title: t("toast.settingsSaved"),
        });
      }
    } catch (err) {
      if (version === saveVersion.current) {
        log.error("settings save failed", err);
        setError(errorMessage(err));
        setSaveState("idle");
      }
    } finally {
      if (version === saveVersion.current) setSaving(false);
    }
  }

  async function syncAutostart(enabled: boolean) {
    if (!isTauriRuntime()) return;
    try {
      const autostart = await import("@tauri-apps/plugin-autostart");
      if (enabled) {
        await autostart.enable();
      } else {
        await autostart.disable();
      }
    } catch (err) {
      log.warn("autostart sync failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: errorMessage(err),
      });
    }
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setDefaultSaveDir(selected);
  }

  async function copyBrowserDiagnostics() {
    if (!browserStatus) return;
    const lines = [
      `Native host: ${browserStatus.nativeHostName}`,
      `Native host path: ${browserStatus.nativeHostPath ?? "(missing)"}`,
      `Extension source: ${browserStatus.extensionCorePath ?? "(missing)"}`,
      ...browserStatus.browsers.map(
        (browser) =>
          `${browser.displayName}: profile=${browser.profile}, extensionId=${browser.extensionId ?? "(none)"}, manifest=${browser.manifestPath ?? "(missing)"}, installed=${browser.manifestInstalled}`,
      ),
    ];
    await navigator.clipboard.writeText(lines.join("\n"));
    addToast({ title: t("settings.browserDiagnosticsCopied"), tone: "success" });
  }

  async function exportBrowserPackages() {
    setBrowserExporting(true);
    try {
      const result = await exportBrowserExtensionPackages();
      setBrowserExportResult(result);
      addToast({
        title: t("settings.browserPackagesExported"),
        description: result.outputDir,
        tone: "success",
      });
    } catch (err) {
      log.error("browser extension package export failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: errorMessage(err),
      });
    } finally {
      setBrowserExporting(false);
    }
  }

  async function updateBrowserCapture(patch: Partial<BrowserCaptureSettings>) {
    setBrowserCaptureSaving(true);
    try {
      const next = await updateBrowserCaptureSettings({
        autoIntercept: patch.autoIntercept ?? null,
        forwardHeaders: patch.forwardHeaders ?? null,
        forwardHeadersMode: patch.forwardHeadersMode ?? null,
        minSizeBytes: patch.minSizeBytes ?? null,
        fileExtensions: patch.fileExtensions ?? null,
        siteRules: patch.siteRules ?? null,
      });
      setBrowserCapture(next);
      setBrowserStatus((current) => (current ? { ...current, capture: next } : current));
    } catch (err) {
      log.error("browser capture settings update failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: errorMessage(err),
      });
    } finally {
      setBrowserCaptureSaving(false);
    }
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
      const status = await getBrowserIntegrationStatus();
      setBrowserStatus(status);
      setBrowserCapture(status.capture ?? (await getBrowserCaptureSettings()));
    } catch (err) {
      log.error("browser integration status refresh failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: errorMessage(err),
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
        description: errorMessage(err),
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
                className="mb-4 rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
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
                tip={t("settings.maxActiveTasksTip")}
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
                tip={t("settings.globalSpeedLimitTip")}
              >
                <ByteUnitInput
                  id="global-speed-limit"
                  valueBytes={globalSpeedLimitBps}
                  onChange={setGlobalSpeedLimitBps}
                  placeholder={t("settings.globalSpeedLimitPlaceholder")}
                  disabled={controlsDisabled}
                  unitAriaLabel={t("settings.speedUnit")}
                  units={[
                    ["1", "B/s"],
                    ["1024", "KB/s"],
                    ["1048576", "MB/s"],
                    ["1073741824", "GB/s"],
                  ]}
                  allowEmpty
                />
              </SettingsRow>

              <SettingsRow
                title={t("settings.multiConnectionThreshold")}
                htmlFor="multi-connection-threshold"
                tip={t("settings.multiConnectionThresholdTip")}
              >
                <ByteUnitInput
                  id="multi-connection-threshold"
                  valueBytes={multiConnectionThresholdBytes}
                  onChange={setMultiConnectionThresholdBytes}
                  disabled={controlsDisabled}
                  unitAriaLabel={t("settings.sizeUnit")}
                  units={[
                    ["1048576", "MB"],
                    ["1073741824", "GB"],
                  ]}
                />
              </SettingsRow>
            </SettingsSection>

            <SettingsSection
              title={t("settings.advancedDownloads")}
              description={t("settings.advancedDownloadsDescription")}
            >
              <SettingsRow
                title={t("settings.segmentCount")}
                htmlFor="segment-count"
                tip={t("settings.segmentCountTip")}
              >
                <Input
                  id="segment-count"
                  type="number"
                  min={1}
                  max={MAX_SEGMENT_COUNT}
                  step={1}
                  value={segmentCount}
                  onChange={(event) => {
                    const next = event.target.valueAsNumber;
                    if (Number.isFinite(next)) {
                      setSegmentCount(Math.min(MAX_SEGMENT_COUNT, Math.max(1, Math.floor(next))));
                    }
                  }}
                  disabled={controlsDisabled}
                  className="h-11 w-28 bg-surface-root text-center font-mono md:h-8"
                />
              </SettingsRow>

              <SettingsRow
                title={t("settings.maxConnectionsPerHost")}
                htmlFor="max-connections-per-host"
                tip={t("settings.maxConnectionsPerHostTip")}
              >
                <Input
                  id="max-connections-per-host"
                  type="number"
                  min={1}
                  max={MAX_CONNECTIONS_PER_HOST}
                  step={1}
                  value={maxConnectionsPerHost}
                  onChange={(event) => {
                    const next = event.target.valueAsNumber;
                    if (Number.isFinite(next)) {
                      setMaxConnectionsPerHost(
                        Math.min(MAX_CONNECTIONS_PER_HOST, Math.max(1, Math.floor(next))),
                      );
                    }
                  }}
                  disabled={controlsDisabled}
                  className="h-11 w-28 bg-surface-root text-center font-mono md:h-8"
                />
              </SettingsRow>
            </SettingsSection>

            <SettingsSection
              title={t("settings.network")}
              description={t("settings.networkDescription")}
            >
              <SettingsRow title={t("settings.proxyMode")} htmlFor="proxy-mode-select">
                <select
                  id="proxy-mode-select"
                  value={proxyMode}
                  onChange={(event) => setProxyMode(event.target.value as AppProxyMode)}
                  className="h-11 w-full max-w-xs rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
                  disabled={controlsDisabled}
                >
                  <option value="off">{t("settings.proxyOff")}</option>
                  <option value="system">{t("settings.proxySystem")}</option>
                  <option value="custom">{t("settings.proxyCustom")}</option>
                </select>
              </SettingsRow>

              <SettingsRow title={t("settings.proxyUrl")} htmlFor="proxy-url">
                <div className="grid gap-2">
                  <Input
                    id="proxy-url"
                    value={proxyUrl}
                    onChange={(event) => setProxyUrl(event.target.value)}
                    placeholder="socks5://127.0.0.1:1080"
                    disabled={controlsDisabled || proxyMode !== "custom"}
                    className="h-11 bg-surface-root font-mono md:h-8"
                  />
                  <p className="text-xs leading-5 text-text-muted">
                    {t("settings.proxyUrlDescription")}
                  </p>
                </div>
              </SettingsRow>

              <SettingsRow title={t("settings.proxyNoProxy")} htmlFor="proxy-no-proxy">
                <Input
                  id="proxy-no-proxy"
                  value={proxyNoProxy}
                  onChange={(event) => setProxyNoProxy(event.target.value)}
                  placeholder="localhost,127.0.0.1,.local"
                  disabled={controlsDisabled || proxyMode !== "custom"}
                  className="h-11 bg-surface-root font-mono md:h-8"
                />
              </SettingsRow>

              <SettingsRow title={t("settings.proxyUsername")} htmlFor="proxy-username">
                <Input
                  id="proxy-username"
                  value={proxyUsername}
                  onChange={(event) => setProxyUsername(event.target.value)}
                  disabled={controlsDisabled || proxyMode !== "custom"}
                  className="h-11 bg-surface-root md:h-8"
                />
              </SettingsRow>

              <SettingsRow title={t("settings.proxyPassword")} htmlFor="proxy-password">
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                  <Input
                    id="proxy-password"
                    type="password"
                    value={proxyPassword}
                    onChange={(event) => {
                      setProxyPassword(event.target.value);
                      setClearProxyPassword(false);
                    }}
                    placeholder={
                      proxyPasswordSaved
                        ? t("settings.proxyPasswordSaved")
                        : t("settings.proxyPasswordPlaceholder")
                    }
                    disabled={controlsDisabled || proxyMode !== "custom"}
                    className="h-11 bg-surface-root md:h-8"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-11 shrink-0 md:h-8"
                    onClick={() => {
                      setProxyPassword("");
                      setProxyPasswordSaved(false);
                      setClearProxyPassword(true);
                    }}
                    disabled={controlsDisabled || proxyMode !== "custom" || !proxyPasswordSaved}
                  >
                    <Trash2 className="h-4 w-4" />
                    {t("settings.proxyClearPassword")}
                  </Button>
                </div>
              </SettingsRow>
            </SettingsSection>

            <SettingsSection title={t("settings.interface")}>
              <SettingsRow title={t("settings.themeMode")} htmlFor="theme-mode-select">
                <select
                  id="theme-mode-select"
                  value={theme ?? "system"}
                  onChange={(event) => setTheme(event.target.value)}
                  className="h-11 w-full max-w-xs rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
                >
                  <option value="system">{t("settings.themeSystem")}</option>
                  <option value="light">{t("settings.themeLight")}</option>
                  <option value="dark">{t("settings.themeDark")}</option>
                </select>
              </SettingsRow>
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
                  <option value="zh-TW">{t("locale.zhTW")}</option>
                  <option value="ja">{t("locale.ja")}</option>
                  <option value="ko">{t("locale.ko")}</option>
                  <option value="ru">{t("locale.ru")}</option>
                  <option value="es">{t("locale.es")}</option>
                </select>
              </SettingsRow>
              <SettingsRow title={t("settings.fontFamily")} htmlFor="font-family-select">
                <select
                  id="font-family-select"
                  value={fontFamily}
                  onChange={(event) => setFontFamily(event.target.value as AppFontFamily)}
                  className="h-11 w-full max-w-xs rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
                  disabled={controlsDisabled}
                >
                  <option value="source_han_sans_sc">
                    {t("settings.fontFamilySourceHanSans")}
                  </option>
                  <option value="system">{t("settings.fontFamilySystem")}</option>
                </select>
              </SettingsRow>
              <SettingsRow title={t("settings.accentColor")} htmlFor="accent-color-picker">
                <div id="accent-color-picker" className="flex flex-wrap items-center gap-2.5">
                  {(
                    [
                      ["blue", t("settings.accentBlue")],
                      ["purple", t("settings.accentPurple")],
                      ["teal", t("settings.accentTeal")],
                      ["green", t("settings.accentGreen")],
                      ["orange", t("settings.accentOrange")],
                      ["rose", t("settings.accentRose")],
                      ["indigo", t("settings.accentIndigo")],
                      ["amber", t("settings.accentAmber")],
                    ] as const
                  ).map(([color, label]) => {
                    const hue = ACCENT_HUES[color as AppAccentColor];
                    const isSelected = accentColor === color;
                    const swatchBg = isSelected
                      ? "var(--accent-primary)"
                      : `oklch(${isDark ? "0.72 0.14" : "0.48 0.18"} ${hue})`;
                    return (
                      <button
                        key={color}
                        type="button"
                        aria-label={label}
                        title={label}
                        disabled={controlsDisabled}
                        onClick={() => setAccentColor(color as AppAccentColor)}
                        className={cn(
                          "h-8 w-8 rounded-full border-2 transition-all",
                          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface-base",
                          "disabled:opacity-50",
                          isSelected
                            ? "border-text-primary scale-110 shadow-md"
                            : "border-border-subtle hover:scale-105 hover:border-text-secondary",
                        )}
                        style={{ backgroundColor: swatchBg }}
                      />
                    );
                  })}
                </div>
              </SettingsRow>
            </SettingsSection>

            <SettingsSection
              title={t("settings.desktopIntegration")}
              description={t("settings.desktopIntegrationDescription")}
            >
              <SettingsToggle
                title={t("settings.systemNotifications")}
                description={t("settings.systemNotificationsDescription")}
                checked={systemNotifications}
                disabled={controlsDisabled}
                onChange={setSystemNotifications}
              />
              <SettingsToggle
                title={t("settings.closeToTray")}
                description={t("settings.closeToTrayDescription")}
                checked={closeToTray}
                disabled={controlsDisabled}
                onChange={setCloseToTray}
              />
              <SettingsToggle
                title={t("settings.startOnBoot")}
                description={t("settings.startOnBootDescription")}
                checked={startOnBoot}
                disabled={controlsDisabled}
                onChange={setStartOnBoot}
              />
              <SettingsToggle
                title={t("settings.floatingWindow")}
                description={t("settings.floatingWindowDescription")}
                checked={floatingWindowEnabled}
                disabled={controlsDisabled}
                onChange={setFloatingWindowEnabled}
              />
            </SettingsSection>

            <SettingsSection
              title={t("settings.browserIntegration")}
              description={t("settings.browserIntegrationDescription")}
            >
              {browserCapture ? (
                <div className="grid border-b border-border-divider">
                  <SettingsToggle
                    title={t("settings.browserAutoIntercept")}
                    description={t("settings.browserAutoInterceptDescription")}
                    checked={browserCapture.autoIntercept}
                    disabled={browserLoading || browserCaptureSaving}
                    onChange={(autoIntercept) => void updateBrowserCapture({ autoIntercept })}
                  />
                  <SettingsToggle
                    title={t("settings.browserForwardHeaders")}
                    description={t("settings.browserForwardHeadersDescription")}
                    checked={browserCapture.forwardHeadersMode === "enabled"}
                    disabled={browserLoading || browserCaptureSaving}
                    onChange={(forwardHeaders) =>
                      void updateBrowserCapture({
                        forwardHeadersMode: forwardHeaders ? "enabled" : "disabled",
                      })
                    }
                  />
                  <SettingsRow
                    title={t("settings.browserForwardHeadersMode")}
                  >
                    <div className="grid gap-1">
                      <select
                        value={browserCapture.forwardHeadersMode}
                        disabled={browserLoading || browserCaptureSaving}
                        aria-label={t("settings.browserForwardHeadersMode")}
                        onChange={(event) =>
                          void updateBrowserCapture({
                            forwardHeadersMode: event.target.value as BrowserForwardHeadersMode,
                          })
                        }
                        className="h-11 w-full max-w-xs rounded-md border border-border-subtle bg-surface-root px-3 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
                      >
                        <option value="ask">{t("settings.browserForwardHeadersAsk")}</option>
                        <option value="enabled">{t("settings.browserForwardHeadersEnabled")}</option>
                        <option value="disabled">{t("settings.browserForwardHeadersDisabled")}</option>
                      </select>
                      <p className="max-w-xl text-xs leading-5 text-text-muted">
                        {t("settings.browserForwardHeadersModeDescription")}
                      </p>
                    </div>
                  </SettingsRow>
                </div>
              ) : null}
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
                      className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)_auto] md:items-center"
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
                        <p className="mt-1 truncate text-xs text-text-muted">
                          {browser.profile} / {browser.extensionId ?? t("settings.browserNoExtensionId")}
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
                <div className="border-t border-border-divider px-4 py-3 text-xs leading-5 text-text-muted">
                  <p>
                    {t("settings.browserHostName")}{" "}
                    <span className="font-mono text-text-secondary">
                      {browserStatus.nativeHostName}
                    </span>
                  </p>
                  <p className="truncate">
                    {t("settings.browserNativeHostPath")}{" "}
                    <span className="font-mono text-text-secondary">
                      {browserStatus.nativeHostPath ?? t("settings.browserNoManifestPath")}
                    </span>
                  </p>
                  <p className="truncate">
                    {t("settings.browserExtensionPath")}{" "}
                    <span className="font-mono text-text-secondary">
                      {browserStatus.extensionCorePath ?? t("settings.browserBuildExtensions")}
                    </span>
                  </p>
                  {browserExportResult ? (
                    <p className="mt-2 truncate">
                      {t("settings.browserExportPath")}{" "}
                      <span className="font-mono text-text-secondary">
                        {browserExportResult.outputDir}
                      </span>
                    </p>
                  ) : null}
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-11 md:h-8"
                      onClick={() => void exportBrowserPackages()}
                      disabled={browserExporting || browserLoading}
                    >
                      {browserExporting ? (
                        <LoaderCircle className="h-4 w-4 animate-spin" />
                      ) : (
                        <Archive className="h-4 w-4" />
                      )}
                      {t("settings.browserExportPackages")}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-11 md:h-8"
                      onClick={() => void copyBrowserDiagnostics()}
                    >
                      <Clipboard className="h-4 w-4" />
                      {t("settings.browserCopyDiagnostics")}
                    </Button>
                  </div>
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
      <div className="overflow-hidden rounded-lg border border-border-panel bg-surface-base/70">
        {children}
      </div>
    </section>
  );
}

function ByteUnitInput({
  id,
  valueBytes,
  onChange,
  units,
  disabled,
  placeholder,
  allowEmpty,
  unitAriaLabel,
}: {
  id: string;
  valueBytes: string;
  onChange: (value: string) => void;
  units: readonly (readonly [string, string])[];
  disabled?: boolean;
  placeholder?: string;
  allowEmpty?: boolean;
  unitAriaLabel?: string;
}) {
  const initialUnit = bestUnit(valueBytes, units);
  const [unit, setUnit] = useState(initialUnit);
  const unitSize = Number(unit);
  const amount = displayAmount(valueBytes, unitSize, allowEmpty);

  function commit(nextAmount: string, nextUnit = unit) {
    const trimmed = nextAmount.trim();
    if (allowEmpty && trimmed === "") {
      onChange("");
      return;
    }
    const parsed = Number(trimmed);
    const parsedUnit = Number(nextUnit);
    if (Number.isFinite(parsed) && parsed >= 0 && Number.isFinite(parsedUnit)) {
      onChange(String(Math.floor(parsed * parsedUnit)));
    }
  }

  return (
    <div className="flex w-full max-w-full gap-2 sm:max-w-xs">
      <Input
        id={id}
        type="number"
        min={0}
        step={unitSize >= 1048576 ? 0.25 : 1}
        value={amount}
        onChange={(event) => commit(event.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        className="h-11 min-w-0 bg-surface-root text-center font-mono md:h-8"
      />
      <select
        value={unit}
        onChange={(event) => {
          setUnit(event.target.value);
          commit(amount, event.target.value);
        }}
        disabled={disabled}
        aria-label={unitAriaLabel}
        className="h-11 rounded-md border border-border-subtle bg-surface-root px-2 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary md:h-8"
      >
        {units.map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
    </div>
  );
}

function bestUnit(valueBytes: string, units: readonly (readonly [string, string])[]): string {
  const bytes = Number(valueBytes);
  if (!Number.isFinite(bytes) || bytes <= 0) return units[0]?.[0] ?? "1";
  const exact = [...units]
    .reverse()
    .find(([unit]) => bytes >= Number(unit) && bytes % Number(unit) === 0);
  return exact?.[0] ?? units[0]?.[0] ?? "1";
}

function displayAmount(
  valueBytes: string,
  unitSize: number,
  allowEmpty?: boolean,
): string {
  if (allowEmpty && valueBytes.trim() === "") return "";
  const bytes = Number(valueBytes);
  if (!Number.isFinite(bytes) || bytes < 0 || !Number.isFinite(unitSize) || unitSize <= 0) {
    return "0";
  }
  const value = bytes / unitSize;
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}

function SettingsToggle({
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
        <span className="mt-1 block text-xs leading-5 text-text-muted">
          {description}
        </span>
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

function SettingsRow({
  title,
  children,
  htmlFor,
  controlClassName,
  tip,
}: {
  title: string;
  children: ReactNode;
  htmlFor?: string;
  controlClassName?: string;
  tip?: string;
}) {
  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] md:items-center">
      <div className="flex items-center gap-1.5">
        <label className="text-sm font-medium text-text-secondary" htmlFor={htmlFor}>
          {title}
        </label>
        {tip ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-text-muted/60 transition-colors hover:text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/50"
                tabIndex={-1}
                aria-label={tip}
              >
                <Info className="h-3.5 w-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top" className="max-w-64 text-balance">
              {tip}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>
      <div className={cn("min-w-0", controlClassName)}>{children}</div>
    </div>
  );
}
