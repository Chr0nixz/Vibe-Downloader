import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import {
  Archive,
  Check,
  Clipboard,
  FolderOpen,
  Info,
  LoaderCircle,
  Puzzle,
  RotateCcw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { useTheme } from "next-themes";
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
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { BrowserCaptureControls } from "@/components/settings/BrowserCaptureControls";
import type {
  AppAccentColor,
  AppFontFamily,
  AppProxyMode,
  AppSettings,
  BrowserCaptureSettings,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  CompletionAction,
} from "@/generated/bindings";
import { setLocale, SUPPORTED_LOCALES, type Locale } from "@/i18n";
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

const AUTO_SAVE_DELAY_MS = 1000;
const MAX_SEGMENT_COUNT = 8;
const MAX_CONNECTIONS_PER_HOST = 16;

const SECTION_IDS = [
  "downloads",
  "advanced-downloads",
  "scheduled-downloads",
  "network",
  "interface",
  "desktop-integration",
  "browser-integration",
] as const;

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

const SettingsSearchContext = createContext<{
  query: string;
  setQuery: (q: string) => void;
}>({ query: "", setQuery: () => {} });

function useSettingsSearch() {
  return useContext(SettingsSearchContext);
}

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
  const [autoResumeOnStartup, setAutoResumeOnStartup] = useState(false);
  const [floatingWindowEnabled, setFloatingWindowEnabled] = useState(false);
  const [clipboardMonitorEnabled, setClipboardMonitorEnabled] = useState(true);
  const [fontFamily, setFontFamily] = useState<AppFontFamily>("source_han_sans_sc");
  const [accentColor, setAccentColor] = useState<AppAccentColor>("blue");
  const [proxyMode, setProxyMode] = useState<AppProxyMode>("off");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyNoProxy, setProxyNoProxy] = useState("");
  const [proxyUsername, setProxyUsername] = useState("");
  const [proxyPassword, setProxyPassword] = useState("");
  const [proxyPasswordSaved, setProxyPasswordSaved] = useState(false);
  const [clearProxyPassword, setClearProxyPassword] = useState(false);
  const [scheduleDownloadWindowEnabled, setScheduleDownloadWindowEnabled] = useState(false);
  const [scheduleDownloadWindowStart, setScheduleDownloadWindowStart] = useState("00:00");
  const [scheduleDownloadWindowEnd, setScheduleDownloadWindowEnd] = useState("06:00");
  const [scheduleSpeedLimitWindowEnabled, setScheduleSpeedLimitWindowEnabled] = useState(false);
  const [scheduleSpeedLimitWindowStart, setScheduleSpeedLimitWindowStart] = useState("18:00");
  const [scheduleSpeedLimitWindowEnd, setScheduleSpeedLimitWindowEnd] = useState("23:00");
  const [scheduleSpeedLimitBps, setScheduleSpeedLimitBps] = useState("");
  const [completionAction, setCompletionAction] = useState<CompletionAction>("none");
  const [completionCountdownSeconds, setCompletionCountdownSeconds] = useState(30);
  const [saving, setSaving] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [showResetDialog, setShowResetDialog] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [settingsSearch, setSettingsSearch] = useState("");
  const [browserStatus, setBrowserStatus] = useState<BrowserIntegrationStatus | null>(null);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserAction, setBrowserAction] = useState<string | null>(null);
  const [browserExporting, setBrowserExporting] = useState(false);
  const [browserExportResult, setBrowserExportResult] =
    useState<BrowserExtensionExportResult | null>(null);
  const [browserCapture, setBrowserCapture] = useState<BrowserCaptureSettings | null>(null);
  const [browserCaptureSaving, setBrowserCaptureSaving] = useState(false);
  const browserExperimentalCaptureEnabled =
    browserStatus?.experimentalCaptureEnabled ?? false;
  const autoSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveVersion = useRef(0);
  const currentLocale = (SUPPORTED_LOCALES.includes(i18n.language as Locale)
    ? (i18n.language as Locale)
    : "en") as Locale;
  const controlsDisabled = loading;

  /* Section navigation */
  const [activeSection, setActiveSection] = useState<string>(SECTION_IDS[0]);
  const sectionNavRef = useRef<HTMLDivElement>(null);
  const isScrollingRef = useRef(false);

  useEffect(() => {
    const nav = sectionNavRef.current;
    if (!nav) return;
    const root = nav.closest<HTMLElement>("[data-settings-scroll]");
    if (!root) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (isScrollingRef.current) return;
        let best: { id: string; ratio: number } | null = null;
        for (const entry of entries) {
          if (
            entry.isIntersecting &&
            entry.intersectionRatio > (best?.ratio ?? 0)
          ) {
            best = { id: entry.target.id, ratio: entry.intersectionRatio };
          }
        }
        if (best) setActiveSection(best.id);
      },
      {
        root,
        rootMargin: "-56px 0px -60% 0px",
        threshold: [0, 0.1, 0.25],
      },
    );

    for (const id of SECTION_IDS) {
      const element = document.getElementById(id);
      if (element) observer.observe(element);
    }

    return () => observer.disconnect();
  }, []);

  const scrollToSection = useCallback((id: string) => {
    setActiveSection(id);
    const element = document.getElementById(id);
    if (!element) return;
    isScrollingRef.current = true;
    const prefersReducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    element.scrollIntoView({
      behavior: prefersReducedMotion ? "instant" : "smooth",
      block: "start",
    });
    window.setTimeout(() => {
      isScrollingRef.current = false;
    }, 800);
  }, []);

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
    setAutoResumeOnStartup(settings.autoResumeOnStartup);
    setFloatingWindowEnabled(settings.floatingWindowEnabled);
    setClipboardMonitorEnabled(settings.clipboardMonitorEnabled);
    setFontFamily(settings.fontFamily);
    setAccentColor(settings.accentColor);
    setProxyMode(settings.proxyMode);
    setProxyUrl(settings.proxyUrl);
    setProxyNoProxy(settings.proxyNoProxy);
    setProxyUsername(settings.proxyUsername);
    setProxyPassword("");
    setProxyPasswordSaved(settings.proxyPasswordSaved);
    setClearProxyPassword(false);
    setScheduleDownloadWindowEnabled(settings.scheduleDownloadWindowEnabled);
    setScheduleDownloadWindowStart(settings.scheduleDownloadWindowStart);
    setScheduleDownloadWindowEnd(settings.scheduleDownloadWindowEnd);
    setScheduleSpeedLimitWindowEnabled(settings.scheduleSpeedLimitWindowEnabled);
    setScheduleSpeedLimitWindowStart(settings.scheduleSpeedLimitWindowStart);
    setScheduleSpeedLimitWindowEnd(settings.scheduleSpeedLimitWindowEnd);
    setScheduleSpeedLimitBps(settings.scheduleSpeedLimitBps ?? "");
    setCompletionAction(settings.completionAction);
    setCompletionCountdownSeconds(settings.completionCountdownSeconds);
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
      autoResumeOnStartup === settings.autoResumeOnStartup &&
      floatingWindowEnabled === settings.floatingWindowEnabled &&
      clipboardMonitorEnabled === settings.clipboardMonitorEnabled &&
      fontFamily === settings.fontFamily &&
      accentColor === settings.accentColor &&
      proxyMode === settings.proxyMode &&
      proxyUrl === settings.proxyUrl &&
      proxyNoProxy === settings.proxyNoProxy &&
      proxyUsername === settings.proxyUsername &&
      scheduleDownloadWindowEnabled === settings.scheduleDownloadWindowEnabled &&
      scheduleDownloadWindowStart === settings.scheduleDownloadWindowStart &&
      scheduleDownloadWindowEnd === settings.scheduleDownloadWindowEnd &&
      scheduleSpeedLimitWindowEnabled === settings.scheduleSpeedLimitWindowEnabled &&
      scheduleSpeedLimitWindowStart === settings.scheduleSpeedLimitWindowStart &&
      scheduleSpeedLimitWindowEnd === settings.scheduleSpeedLimitWindowEnd &&
      scheduleSpeedLimitBps === (settings.scheduleSpeedLimitBps ?? "") &&
      completionAction === settings.completionAction &&
      completionCountdownSeconds === settings.completionCountdownSeconds &&
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
        autoResumeOnStartup,
        floatingWindowEnabled,
        clipboardMonitorEnabled,
        fontFamily,
        accentColor,
        proxyMode,
        proxyUrl,
        proxyNoProxy,
        proxyUsername,
        proxyPasswordSaved,
        scheduleDownloadWindowEnabled,
        scheduleDownloadWindowStart,
        scheduleDownloadWindowEnd,
        scheduleSpeedLimitWindowEnabled,
        scheduleSpeedLimitWindowStart,
        scheduleSpeedLimitWindowEnd,
        scheduleSpeedLimitBps: scheduleSpeedLimitBps.trim() || null,
        completionAction,
        completionCountdownSeconds,
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
    autoResumeOnStartup,
    floatingWindowEnabled,
    clipboardMonitorEnabled,
    fontFamily,
    accentColor,
    proxyMode,
    proxyUrl,
    proxyNoProxy,
    proxyUsername,
    proxyPassword,
    proxyPasswordSaved,
    clearProxyPassword,
    scheduleDownloadWindowEnabled,
    scheduleDownloadWindowStart,
    scheduleDownloadWindowEnd,
    scheduleSpeedLimitWindowEnabled,
    scheduleSpeedLimitWindowStart,
    scheduleSpeedLimitWindowEnd,
    scheduleSpeedLimitBps,
    completionAction,
    completionCountdownSeconds,
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
        autoResumeOnStartup: nextSettings.autoResumeOnStartup,
        floatingWindowEnabled: nextSettings.floatingWindowEnabled,
        clipboardMonitorEnabled: nextSettings.clipboardMonitorEnabled,
        fontFamily: nextSettings.fontFamily,
        accentColor: nextSettings.accentColor,
        proxyMode: nextSettings.proxyMode,
        proxyUrl: nextSettings.proxyUrl,
        proxyNoProxy: nextSettings.proxyNoProxy,
        proxyUsername: nextSettings.proxyUsername,
        proxyPassword: proxyPassword.trim() || null,
        clearProxyPassword,
        scheduleDownloadWindowEnabled: nextSettings.scheduleDownloadWindowEnabled,
        scheduleDownloadWindowStart: nextSettings.scheduleDownloadWindowStart,
        scheduleDownloadWindowEnd: nextSettings.scheduleDownloadWindowEnd,
        scheduleSpeedLimitWindowEnabled: nextSettings.scheduleSpeedLimitWindowEnabled,
        scheduleSpeedLimitWindowStart: nextSettings.scheduleSpeedLimitWindowStart,
        scheduleSpeedLimitWindowEnd: nextSettings.scheduleSpeedLimitWindowEnd,
        scheduleSpeedLimitBps: nextSettings.scheduleSpeedLimitBps,
        completionAction: nextSettings.completionAction,
        completionCountdownSeconds: nextSettings.completionCountdownSeconds,
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

  async function handleResetDefaults() {
    setResetting(true);
    try {
      const updated = await updateSettings({
        maxActiveTasks: 2,
        defaultSaveDir: null,
        globalSpeedLimitBps: null,
        multiConnectionThresholdBytes: "1048576",
        segmentCount: 4,
        maxConnectionsPerHost: 8,
        systemNotifications: true,
        closeToTray: false,
        startOnBoot: false,
        autoResumeOnStartup: false,
        floatingWindowEnabled: false,
        clipboardMonitorEnabled: true,
        fontFamily: "source_han_sans_sc",
        accentColor: "blue",
        proxyMode: "off",
        proxyUrl: "",
        proxyNoProxy: "",
        proxyUsername: "",
        proxyPassword: "",
        clearProxyPassword: true,
        scheduleDownloadWindowEnabled: false,
        scheduleDownloadWindowStart: "00:00",
        scheduleDownloadWindowEnd: "06:00",
        scheduleSpeedLimitWindowEnabled: false,
        scheduleSpeedLimitWindowStart: "18:00",
        scheduleSpeedLimitWindowEnd: "23:00",
        scheduleSpeedLimitBps: null,
        completionAction: "none",
        completionCountdownSeconds: 30,
      });
      setSettings(updated);
      setDefaultSaveDir(updated.defaultSaveDir);
      setMaxActiveTasks(updated.maxActiveTasks);
      setGlobalSpeedLimitBps(
        updated.globalSpeedLimitBps ? String(updated.globalSpeedLimitBps) : "",
      );
      setMultiConnectionThresholdBytes(
        updated.multiConnectionThresholdBytes
          ? String(updated.multiConnectionThresholdBytes)
          : "",
      );
      setSegmentCount(updated.segmentCount);
      setMaxConnectionsPerHost(updated.maxConnectionsPerHost);
      setSystemNotifications(updated.systemNotifications);
      setCloseToTray(updated.closeToTray);
      setStartOnBoot(updated.startOnBoot);
      setAutoResumeOnStartup(updated.autoResumeOnStartup);
      setFloatingWindowEnabled(updated.floatingWindowEnabled);
      setClipboardMonitorEnabled(updated.clipboardMonitorEnabled);
      setFontFamily(updated.fontFamily);
      setAccentColor(updated.accentColor);
      setProxyMode(updated.proxyMode);
      setProxyUrl(updated.proxyUrl);
      setProxyNoProxy(updated.proxyNoProxy);
      setProxyUsername(updated.proxyUsername);
      setProxyPassword("");
      setProxyPasswordSaved(updated.proxyPasswordSaved);
      setClearProxyPassword(false);
      setScheduleDownloadWindowEnabled(updated.scheduleDownloadWindowEnabled);
      setScheduleDownloadWindowStart(updated.scheduleDownloadWindowStart);
      setScheduleDownloadWindowEnd(updated.scheduleDownloadWindowEnd);
      setScheduleSpeedLimitWindowEnabled(updated.scheduleSpeedLimitWindowEnabled);
      setScheduleSpeedLimitWindowStart(updated.scheduleSpeedLimitWindowStart);
      setScheduleSpeedLimitWindowEnd(updated.scheduleSpeedLimitWindowEnd);
      setScheduleSpeedLimitBps(
        updated.scheduleSpeedLimitBps
          ? String(updated.scheduleSpeedLimitBps)
          : "",
      );
      setCompletionAction(updated.completionAction);
      setCompletionCountdownSeconds(updated.completionCountdownSeconds);
      addToast({
        title: t("settings.resetDefaults"),
        tone: "success",
      });
      setShowResetDialog(false);
    } catch (err) {
      log.error("reset defaults failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: errorMessage(err),
      });
    } finally {
      setResetting(false);
    }
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
          <div className="flex shrink-0 items-center gap-3">
            <SaveStatus
              state={saveState}
              saving={saving}
              className="hidden sm:inline-flex"
            />
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowResetDialog(true)}
              className="gap-1.5"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              {t("settings.resetDefaults")}
            </Button>
          </div>
        </div>

        <SettingsSearchContext.Provider value={{ query: settingsSearch, setQuery: setSettingsSearch }}>
        <div
          ref={sectionNavRef}
          data-settings-scroll
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
        >
          <nav
            className="sticky top-0 z-10 border-b border-border-subtle bg-surface-base/85 backdrop-blur-md"
            role="navigation"
            aria-label={t("settings.sectionsNav")}
          >
            <div className="mx-auto max-w-4xl px-3 pt-2 sm:px-4 md:px-6">
              <div className="relative flex items-center">
                <Search className="pointer-events-none absolute left-3 h-4 w-4 text-text-muted" />
                <Input
                  value={settingsSearch}
                  onChange={(e) => setSettingsSearch(e.target.value)}
                  placeholder={t("settings.searchSettings")}
                  className="h-9 w-full rounded-md border border-border-subtle bg-surface-root pl-9 pr-8 text-sm text-text-primary placeholder:text-text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                />
                {settingsSearch ? (
                  <button
                    type="button"
                    onClick={() => setSettingsSearch("")}
                    className="absolute right-2 rounded p-0.5 text-text-muted hover:text-text-secondary"
                    aria-label="Clear search"
                  >
                    <X className="h-4 w-4" />
                  </button>
                ) : null}
              </div>
            </div>
            <div className="mx-auto flex max-w-4xl gap-1 overflow-x-auto px-3 sm:px-4 md:px-6">
              {(
                [
                  ["downloads", t("settings.downloads")],
                  ["advanced-downloads", t("settings.advancedDownloads")],
                  ["scheduled-downloads", t("settings.scheduledDownloads")],
                  ["network", t("settings.network")],
                  ["interface", t("settings.interface")],
                  ["desktop-integration", t("settings.desktopIntegration")],
                  ["browser-integration", t("settings.browserIntegration")],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => scrollToSection(id)}
                  className={cn(
                    "relative shrink-0 px-3 py-2.5 text-[13px] font-medium transition-colors",
                    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary",
                    activeSection === id
                      ? "text-accent-primary"
                      : "text-text-muted hover:text-text-secondary",
                  )}
                >
                  {label}
                  {activeSection === id ? (
                    <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-accent-primary" />
                  ) : null}
                </button>
              ))}
            </div>
          </nav>
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
              id="downloads"
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
              id="advanced-downloads"
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
              id="scheduled-downloads"
              title={t("settings.scheduledDownloads")}
              description={t("settings.scheduledDownloadsDescription")}
            >
              <SettingsSubHeading title={t("settings.scheduleGroupDownload")} />
              <SettingsToggle
                title={t("settings.downloadWindow")}
                description={t("settings.downloadWindowDescription")}
                checked={scheduleDownloadWindowEnabled}
                disabled={controlsDisabled}
                onChange={setScheduleDownloadWindowEnabled}
              />
              <SettingsRow title={t("settings.downloadWindowTime")} controlClassName="max-w-sm" tip={t("settings.downloadWindowTimeTip")}>
                <div className="flex items-center gap-2">
                  <Input
                    type="time"
                    value={scheduleDownloadWindowStart}
                    onChange={(event) => setScheduleDownloadWindowStart(event.target.value)}
                    disabled={controlsDisabled || !scheduleDownloadWindowEnabled}
                    className="h-11 w-32 bg-surface-root font-mono md:h-8"
                  />
                  <span className="text-xs text-text-muted">-</span>
                  <Input
                    type="time"
                    value={scheduleDownloadWindowEnd}
                    onChange={(event) => setScheduleDownloadWindowEnd(event.target.value)}
                    disabled={controlsDisabled || !scheduleDownloadWindowEnabled}
                    className="h-11 w-32 bg-surface-root font-mono md:h-8"
                  />
                </div>
              </SettingsRow>
              <SettingsSubHeading title={t("settings.scheduleGroupSpeed")} />
              <SettingsToggle
                title={t("settings.speedLimitWindow")}
                description={t("settings.speedLimitWindowDescription")}
                checked={scheduleSpeedLimitWindowEnabled}
                disabled={controlsDisabled}
                onChange={setScheduleSpeedLimitWindowEnabled}
              />
              <SettingsRow title={t("settings.speedLimitWindowTime")} controlClassName="max-w-sm" tip={t("settings.speedLimitWindowTimeTip")}>
                <div className="flex flex-wrap items-center gap-2">
                  <Input
                    type="time"
                    value={scheduleSpeedLimitWindowStart}
                    onChange={(event) => setScheduleSpeedLimitWindowStart(event.target.value)}
                    disabled={controlsDisabled || !scheduleSpeedLimitWindowEnabled}
                    className="h-11 w-32 bg-surface-root font-mono md:h-8"
                  />
                  <span className="text-xs text-text-muted">-</span>
                  <Input
                    type="time"
                    value={scheduleSpeedLimitWindowEnd}
                    onChange={(event) => setScheduleSpeedLimitWindowEnd(event.target.value)}
                    disabled={controlsDisabled || !scheduleSpeedLimitWindowEnabled}
                    className="h-11 w-32 bg-surface-root font-mono md:h-8"
                  />
                </div>
              </SettingsRow>
              <SettingsRow title={t("settings.scheduledSpeedLimit")} htmlFor="scheduled-speed-limit" tip={t("settings.scheduledSpeedLimitTip")}>
                <ByteUnitInput
                  id="scheduled-speed-limit"
                  valueBytes={scheduleSpeedLimitBps}
                  onChange={setScheduleSpeedLimitBps}
                  placeholder={t("settings.globalSpeedLimitPlaceholder")}
                  disabled={controlsDisabled || !scheduleSpeedLimitWindowEnabled}
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
              <SettingsSubHeading title={t("settings.scheduleGroupCompletion")} />
              <SettingsRow title={t("settings.completionAction")} htmlFor="completion-action" tip={t("settings.completionActionTip")}>
                <Select
                  value={completionAction}
                  onValueChange={(value) => setCompletionAction(value as CompletionAction)}
                  disabled={controlsDisabled}
                >
                  <SelectTrigger id="completion-action" className="w-full max-w-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t("settings.completionNone")}</SelectItem>
                    <SelectItem value="exit_app">{t("settings.completionExit")}</SelectItem>
                    <SelectItem value="shutdown">{t("settings.completionShutdown")}</SelectItem>
                  </SelectContent>
                </Select>
              </SettingsRow>
              <SettingsRow title={t("settings.completionCountdown")} htmlFor="completion-countdown" tip={t("settings.completionCountdownTip")}>
                <Input
                  id="completion-countdown"
                  type="number"
                  min={5}
                  max={300}
                  step={5}
                  value={completionCountdownSeconds}
                  onChange={(event) => {
                    const next = event.target.valueAsNumber;
                    if (Number.isFinite(next)) {
                      setCompletionCountdownSeconds(Math.min(300, Math.max(5, Math.floor(next))));
                    }
                  }}
                  disabled={controlsDisabled || completionAction === "none"}
                  className="h-11 w-28 bg-surface-root text-center font-mono md:h-8"
                />
              </SettingsRow>
            </SettingsSection>

            <SettingsSection
              id="network"
              title={t("settings.network")}
              description={t("settings.networkDescription")}
            >
              <SettingsRow title={t("settings.proxyMode")} htmlFor="proxy-mode-select" tip={t("settings.proxyModeTip")}>
                <Select
                  value={proxyMode}
                  onValueChange={(value) => setProxyMode(value as AppProxyMode)}
                  disabled={controlsDisabled}
                >
                  <SelectTrigger id="proxy-mode-select" className="w-full max-w-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="off">{t("settings.proxyOff")}</SelectItem>
                    <SelectItem value="system">{t("settings.proxySystem")}</SelectItem>
                    <SelectItem value="custom">{t("settings.proxyCustom")}</SelectItem>
                  </SelectContent>
                </Select>
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

              <SettingsRow title={t("settings.proxyNoProxy")} htmlFor="proxy-no-proxy" tip={t("settings.proxyNoProxyTip")}>
                <Input
                  id="proxy-no-proxy"
                  value={proxyNoProxy}
                  onChange={(event) => setProxyNoProxy(event.target.value)}
                  placeholder="localhost,127.0.0.1,.local"
                  disabled={controlsDisabled || proxyMode !== "custom"}
                  className="h-11 bg-surface-root font-mono md:h-8"
                />
              </SettingsRow>

              <SettingsRow title={t("settings.proxyUsername")} htmlFor="proxy-username" tip={t("settings.proxyAuthTip")}>
                <Input
                  id="proxy-username"
                  value={proxyUsername}
                  onChange={(event) => setProxyUsername(event.target.value)}
                  disabled={controlsDisabled || proxyMode !== "custom"}
                  className="h-11 bg-surface-root md:h-8"
                />
              </SettingsRow>

              <SettingsRow title={t("settings.proxyPassword")} htmlFor="proxy-password" tip={t("settings.proxyPasswordTip")}>
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

            <SettingsSection id="interface" title={t("settings.interface")}>
              <SettingsRow title={t("settings.themeMode")} htmlFor="theme-mode-select">
                <Select value={theme ?? "system"} onValueChange={(value) => setTheme(value)}>
                  <SelectTrigger id="theme-mode-select" className="w-full max-w-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="system">{t("settings.themeSystem")}</SelectItem>
                    <SelectItem value="light">{t("settings.themeLight")}</SelectItem>
                    <SelectItem value="dark">{t("settings.themeDark")}</SelectItem>
                  </SelectContent>
                </Select>
              </SettingsRow>
              <SettingsRow title={t("locale.label")} htmlFor="locale-select">
                <Select
                  value={currentLocale}
                  onValueChange={(value) => setLocale(value as Locale)}
                  disabled={controlsDisabled}
                >
                  <SelectTrigger id="locale-select" className="w-full max-w-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="en">{t("locale.en")}</SelectItem>
                    <SelectItem value="zh-CN">{t("locale.zhCN")}</SelectItem>
                    <SelectItem value="zh-TW">{t("locale.zhTW")}</SelectItem>
                    <SelectItem value="ja">{t("locale.ja")}</SelectItem>
                    <SelectItem value="ko">{t("locale.ko")}</SelectItem>
                    <SelectItem value="ru">{t("locale.ru")}</SelectItem>
                    <SelectItem value="es">{t("locale.es")}</SelectItem>
                  </SelectContent>
                </Select>
              </SettingsRow>
              <SettingsRow title={t("settings.fontFamily")} htmlFor="font-family-select">
                <Select
                  value={fontFamily}
                  onValueChange={(value) => setFontFamily(value as AppFontFamily)}
                  disabled={controlsDisabled}
                >
                  <SelectTrigger id="font-family-select" className="w-full max-w-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="source_han_sans_sc">
                      {t("settings.fontFamilySourceHanSans")}
                    </SelectItem>
                    <SelectItem value="system">{t("settings.fontFamilySystem")}</SelectItem>
                  </SelectContent>
                </Select>
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
              id="desktop-integration"
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
                title={t("settings.autoResumeOnStartup")}
                description={t("settings.autoResumeOnStartupDescription")}
                checked={autoResumeOnStartup}
                disabled={controlsDisabled}
                onChange={setAutoResumeOnStartup}
              />
              <SettingsToggle
                title={t("settings.floatingWindow")}
                description={t("settings.floatingWindowDescription")}
                checked={floatingWindowEnabled}
                disabled={controlsDisabled}
                onChange={setFloatingWindowEnabled}
              />
              <SettingsToggle
                title={t("settings.clipboardMonitor")}
                description={t("settings.clipboardMonitorDescription")}
                checked={clipboardMonitorEnabled}
                disabled={controlsDisabled}
                onChange={setClipboardMonitorEnabled}
              />
            </SettingsSection>

            <SettingsSection
              id="browser-integration"
              title={t("settings.browserIntegration")}
              description={t("settings.browserIntegrationDescription")}
            >
              <p className="border-b border-border-divider px-4 py-3 text-xs leading-5 text-text-muted">
                {t("settings.browserIntegrationTip")}
              </p>
              {browserCapture && browserExperimentalCaptureEnabled ? (
                <BrowserCaptureControls
                  settings={browserCapture}
                  disabled={browserLoading || browserCaptureSaving}
                  onUpdate={(patch) => void updateBrowserCapture(patch)}
                />
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
        </div>
        </SettingsSearchContext.Provider>

        <footer className="flex shrink-0 justify-end border-t border-border-subtle bg-surface-base/60 px-4 py-3 sm:hidden">
          <SaveStatus state={saveState} saving={saving} className="w-full justify-center" />
        </footer>

        <Dialog open={showResetDialog} onOpenChange={setShowResetDialog}>
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle>{t("settings.resetDefaultsTitle")}</DialogTitle>
              <DialogDescription>
                {t("settings.resetDefaultsConfirm")}
              </DialogDescription>
            </DialogHeader>
            <DialogBody />
            <DialogFooter className="gap-2 sm:gap-2">
              <Button
                variant="outline"
                onClick={() => setShowResetDialog(false)}
                disabled={resetting}
              >
                {t("actions.cancel")}
              </Button>
              <Button
                variant="danger"
                onClick={handleResetDefaults}
                disabled={resetting}
              >
                {resetting ? (
                  <LoaderCircle className="mr-1.5 h-4 w-4 animate-spin" />
                ) : (
                  <RotateCcw className="mr-1.5 h-4 w-4" />
                )}
                {t("settings.resetDefaults")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
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
  id,
  title,
  description,
  children,
}: {
  id?: string;
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section id={id} className="scroll-mt-12 border-t border-border-subtle py-5 first:border-t-0 first:pt-0">
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
      <Select
        value={unit}
        onValueChange={(value) => {
          setUnit(value);
          commit(amount, value);
        }}
        disabled={disabled}
      >
        <SelectTrigger aria-label={unitAriaLabel} className="w-auto min-w-[4rem] px-2">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {units.map(([value, label]) => (
            <SelectItem key={value} value={value}>
              {label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
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
  const { query } = useSettingsSearch();
  const matchesSearch = !query ||
    title.toLowerCase().includes(query.toLowerCase()) ||
    description.toLowerCase().includes(query.toLowerCase());
  if (!matchesSearch) return null;
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
        className="h-6 w-6 accent-accent-primary"
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
  const { query } = useSettingsSearch();
  const matchesSearch = !query ||
    title.toLowerCase().includes(query.toLowerCase()) ||
    (tip ?? "").toLowerCase().includes(query.toLowerCase());
  if (!matchesSearch) return null;
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
                className="inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-text-muted transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/50"
                tabIndex={0}
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

function SettingsSubHeading({ title }: { title: string }) {
  return (
    <div className="border-t border-border-subtle bg-surface-root/40 px-4 py-2">
      <h4 className="text-xs font-medium uppercase tracking-wide text-text-muted">
        {title}
      </h4>
    </div>
  );
}
