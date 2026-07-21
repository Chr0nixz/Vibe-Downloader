import {
  Archive,
  Check,
  ChevronDown,
  Clipboard,
  FolderOpen,
  Info,
  LoaderCircle,
  Puzzle,
  RotateCcw,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useTheme } from "next-themes";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { BrowserCaptureControls } from "@/components/settings/BrowserCaptureControls";
import { ClassificationRulesEditor } from "@/components/settings/ClassificationRulesEditor";
import { EnvironmentPanel } from "@/components/settings/EnvironmentPanel";
import { SftpKnownHostsEditor } from "@/components/settings/SftpKnownHostsEditor";
import {
  type SettingsSearchSection,
  settingsSearchHasResults,
  settingsSectionMatchesQuery,
} from "@/components/settings/settings-search";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type {
  AppAccentColor,
  AppProxyMode,
  AppSettings,
  BrowserCaptureSettings,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  CompletionAction,
} from "@/generated/bindings";
import { useAppUpdater } from "@/hooks/use-app-updater";
import { LOCALE_LABEL_KEYS, type Locale, STABLE_LOCALES, SUPPORTED_LOCALES, setLocale } from "@/i18n";
import {
  BROWSER_CAPTURE_SAVE_DEBOUNCE_MS,
  captureSettingsEqual,
  mergeBrowserCapturePatch,
  resolveCaptureDraftAfterSave,
} from "@/lib/browser-capture-draft";
import { localizedErrorMessage } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import {
  exportBrowserExtensionPackages,
  getBrowserCaptureSettings,
  getBrowserIntegrationStatus,
  getSettings,
  installBrowserIntegration,
  isTauriRuntime,
  onBrowserIntegrationChanged,
  openDirectoryPicker,
  openFilePicker,
  probeFfmpegVersion,
  uninstallBrowserIntegration,
  updateBrowserCaptureSettings,
  updateSettings,
} from "@/lib/tauri";
import { cn, formatBytes, formatSpeed } from "@/lib/utils";

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
  "environment",
  "external-tools",
  "data-backup",
  "about-updates",
] as const;

type SettingsSectionId = (typeof SECTION_IDS)[number];

const DEFAULT_EXPANDED_SETTINGS_SECTIONS = new Set<SettingsSectionId>([
  "downloads",
  "interface",
  "desktop-integration",
  "about-updates",
]);

const ACCENT_SWATCHES: Record<string, { light: string; dark: string }> = {
  blue: { light: "oklch(0.4 0.18 235)", dark: "oklch(0.76 0.14 235)" },
  purple: { light: "oklch(0.4 0.18 290)", dark: "oklch(0.76 0.14 290)" },
  teal: { light: "oklch(0.4 0.15 190)", dark: "oklch(0.76 0.14 190)" },
  green: { light: "oklch(0.4 0.16 150)", dark: "oklch(0.76 0.14 150)" },
  orange: { light: "oklch(0.4 0.16 55)", dark: "oklch(0.76 0.14 55)" },
  rose: { light: "oklch(0.4 0.18 350)", dark: "oklch(0.76 0.14 350)" },
  indigo: { light: "oklch(0.4 0.18 265)", dark: "oklch(0.76 0.14 265)" },
  amber: { light: "oklch(0.4 0.16 80)", dark: "oklch(0.76 0.14 80)" },
};

const SettingsSearchContext = createContext<{
  query: string;
  setQuery: (q: string) => void;
  forceVisible?: boolean;
}>({ query: "", setQuery: () => {}, forceVisible: false });

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
  const updater = useAppUpdater();
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
  const [accentColor, setAccentColor] = useState<AppAccentColor>("blue");
  const [titlebarGradientEnabled, setTitlebarGradientEnabled] = useState(false);
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
  const [completionRunCommand, setCompletionRunCommand] = useState("");
  const [deleteToTrash, setDeleteToTrash] = useState(true);
  const [autoUpdateCheckEnabled, setAutoUpdateCheckEnabled] = useState(true);
  const [ffmpegPath, setFfmpegPath] = useState("");
  const [ffmpegVersion, setFfmpegVersion] = useState<string | null>(null);
  const [ffmpegProbeError, setFfmpegProbeError] = useState<string | null>(null);
  const [backupBusy, setBackupBusy] = useState(false);
  const [ffmpegProbing, setFfmpegProbing] = useState(false);
  // F-7: Global BitTorrent upload speed limit (bytes/sec). Empty = unlimited.
  const [btUploadLimitBps, setBtUploadLimitBps] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [showResetDialog, setShowResetDialog] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [settingsSearch, setSettingsSearch] = useState("");
  const [browserStatus, setBrowserStatus] = useState<BrowserIntegrationStatus | null>(null);
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserAction, setBrowserAction] = useState<string | null>(null);
  const [browserExporting, setBrowserExporting] = useState(false);
  const [browserExportResult, setBrowserExportResult] = useState<BrowserExtensionExportResult | null>(null);
  const [browserCapture, setBrowserCapture] = useState<BrowserCaptureSettings | null>(null);
  const [browserCaptureDraft, setBrowserCaptureDraft] = useState<BrowserCaptureSettings | null>(null);
  const [browserCaptureSaving, setBrowserCaptureSaving] = useState(false);
  const browserCaptureSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const browserCaptureSaveEpoch = useRef(0);
  const autoSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveVersion = useRef(0);
  const currentLocale = (
    SUPPORTED_LOCALES.includes(i18n.language as Locale) ? (i18n.language as Locale) : "en"
  ) as Locale;
  const controlsDisabled = loading;
  const parsedGlobalSpeedLimit = Number(globalSpeedLimitBps);
  const speedLimitSummary =
    globalSpeedLimitBps.trim() && Number.isFinite(parsedGlobalSpeedLimit)
      ? formatSpeed(parsedGlobalSpeedLimit)
      : t("settings.speedUnlimited");
  const proxyModeLabel =
    proxyMode === "system"
      ? t("settings.proxySystem")
      : proxyMode === "custom"
        ? t("settings.proxyCustom")
        : t("settings.proxyOff");
  const themeLabel =
    theme === "light"
      ? t("settings.themeLight")
      : theme === "dark"
        ? t("settings.themeDark")
        : t("settings.themeSystem");
  const currentLocaleKey = currentLocale === "zh-CN" ? "zhCN" : currentLocale;
  const localeLabel = t(`locale.${currentLocaleKey}`);
  const enabledDesktopIntegrations = [
    systemNotifications,
    closeToTray,
    startOnBoot,
    autoResumeOnStartup,
    floatingWindowEnabled,
    clipboardMonitorEnabled,
  ].filter(Boolean).length;
  const enabledSchedules = [
    scheduleDownloadWindowEnabled,
    scheduleSpeedLimitWindowEnabled,
    completionAction !== "none",
  ].filter(Boolean).length;
  const installedBrowserCount = browserStatus?.browsers.filter((browser) => browser.manifestInstalled).length ?? 0;
  const settingsSections = useMemo<SettingsSearchSection[]>(
    () => [
      {
        id: "downloads",
        title: t("settings.downloads"),
        description: t("settings.downloadsDescription"),
        summary: t("settings.downloadsSummary", {
          count: maxActiveTasks,
          speed: speedLimitSummary,
        }),
        terms: [
          t("settings.defaultSaveDir"),
          t("settings.maxActiveTasks"),
          t("settings.maxActiveTasksTip"),
          t("settings.globalSpeedLimit"),
          t("settings.globalSpeedLimitTip"),
          t("settings.multiConnectionThreshold"),
          t("settings.multiConnectionThresholdTip"),
          t("settings.deleteToTrash"),
          t("settings.deleteToTrashTip"),
        ],
      },
      {
        id: "advanced-downloads",
        title: t("settings.advancedDownloads"),
        description: t("settings.advancedDownloadsDescription"),
        summary: t("settings.advancedDownloadsSummary", {
          segments: segmentCount,
          connections: maxConnectionsPerHost,
        }),
        terms: [
          t("settings.segmentCount"),
          t("settings.segmentCountTip"),
          t("settings.maxConnectionsPerHost"),
          t("settings.maxConnectionsPerHostTip"),
        ],
      },
      {
        id: "scheduled-downloads",
        title: t("settings.scheduledDownloads"),
        description: t("settings.scheduledDownloadsDescription"),
        summary:
          enabledSchedules > 0
            ? t("settings.scheduledDownloadsSummary", { count: enabledSchedules })
            : t("settings.scheduledDownloadsOff"),
        terms: [
          t("settings.scheduleGroupDownload"),
          t("settings.downloadWindow"),
          t("settings.downloadWindowDescription"),
          t("settings.downloadWindowTime"),
          t("settings.scheduleGroupSpeed"),
          t("settings.speedLimitWindow"),
          t("settings.speedLimitWindowDescription"),
          t("settings.speedLimitWindowTime"),
          t("settings.scheduledSpeedLimit"),
          t("settings.scheduleGroupCompletion"),
          t("settings.completionAction"),
          t("settings.completionNone"),
          t("settings.completionExit"),
          t("settings.completionShutdown"),
          t("settings.completionCountdown"),
        ],
      },
      {
        id: "network",
        title: t("settings.network"),
        description: t("settings.networkDescription"),
        summary: proxyModeLabel,
        terms: [
          t("settings.proxyMode"),
          t("settings.proxyModeTip"),
          t("settings.proxyOff"),
          t("settings.proxySystem"),
          t("settings.proxyCustom"),
          t("settings.proxyUrl"),
          t("settings.proxyUrlDescription"),
          t("settings.proxyNoProxy"),
          t("settings.proxyUsername"),
          t("settings.proxyPassword"),
          t("settings.sftpKnownHosts"),
          t("settings.sftpKnownHostsTip"),
          t("settings.sftpKnownHostsForget"),
        ],
      },
      {
        id: "interface",
        title: t("settings.interface"),
        summary: t("settings.interfaceSummary", {
          theme: themeLabel,
          locale: localeLabel,
        }),
        terms: [
          t("settings.themeMode"),
          t("settings.themeSystem"),
          t("settings.themeLight"),
          t("settings.themeDark"),
          t("locale.label"),
          t("locale.en"),
          t("locale.zhCN"),
          t("locale.zhTW"),
          t("locale.ja"),
          t("locale.ko"),
          t("locale.ru"),
          t("locale.es"),
          t("settings.accentColor"),
        ],
      },
      {
        id: "desktop-integration",
        title: t("settings.desktopIntegration"),
        description: t("settings.desktopIntegrationDescription"),
        summary: t("settings.desktopIntegrationSummary", {
          count: enabledDesktopIntegrations,
        }),
        terms: [
          t("settings.systemNotifications"),
          t("settings.systemNotificationsDescription"),
          t("settings.closeToTray"),
          t("settings.closeToTrayDescription"),
          t("settings.startOnBoot"),
          t("settings.startOnBootDescription"),
          t("settings.autoResumeOnStartup"),
          t("settings.autoResumeOnStartupDescription"),
          t("settings.floatingWindow"),
          t("settings.floatingWindowDescription"),
          t("settings.clipboardMonitor"),
          t("settings.clipboardMonitorDescription"),
        ],
      },
      {
        id: "browser-integration",
        title: t("settings.browserIntegration"),
        description: t("settings.browserIntegrationDescription"),
        summary: browserStatus
          ? installedBrowserCount > 0
            ? t("settings.browserIntegrationSummary", { count: installedBrowserCount })
            : t("settings.browserIntegrationSummaryNone")
          : t("settings.browserIntegrationSummaryLoading"),
        terms: [
          t("settings.browserIntegrationTip"),
          t("settings.browserAutoIntercept"),
          t("settings.browserForwardHeaders"),
          t("settings.browserInstalled"),
          t("settings.browserDetected"),
          t("settings.browserNotDetected"),
          t("settings.browserInstall"),
          t("settings.browserUninstall"),
          t("settings.browserHostName"),
          t("settings.browserNativeHostPath"),
          t("settings.browserExtensionPath"),
          t("settings.browserExportPackages"),
          ...(browserStatus?.browsers.map((browser) => browser.displayName) ?? []),
        ],
      },
      {
        id: "environment",
        title: t("settings.environment"),
        description: t("settings.environmentDescription"),
        summary: t("settings.environmentSectionSummary"),
        terms: [
          t("settings.environmentRunCheck"),
          t("settings.environmentCopyReport"),
          t("settings.environmentFixSafe"),
          t("settings.environmentItemNativeHost"),
          t("settings.environmentItemBrowser"),
          t("settings.environmentItemFfmpeg"),
          t("settings.environmentItemProxy"),
          t("settings.environmentItemSaveDir"),
          t("settings.environmentItemDisk"),
          t("settings.environmentItemDatabase"),
          t("settings.environmentItemUpdater"),
          t("settings.browserCopyDiagnostics"),
          "ffmpeg",
          "proxy",
          "disk",
          "Native Messaging",
        ],
      },
      {
        id: "external-tools",
        title: t("settings.externalTools"),
        description: t("settings.externalToolsDescription"),
        summary: ffmpegVersion
          ? t("settings.externalToolsSummary", {
              status: t("settings.ffmpegPath.detected", { version: ffmpegVersion }),
            })
          : ffmpegProbeError
            ? t("settings.externalToolsSummary", {
                status: t("settings.ffmpegPath.invalid", { error: ffmpegProbeError }),
              })
            : ffmpegPath.trim()
              ? t("settings.externalToolsSummary", { status: t("settings.ffmpegPath.detect") })
              : t("settings.externalToolsSummaryMissing"),
        terms: [
          t("settings.ffmpegPath.label"),
          t("settings.ffmpegPath.description"),
          t("settings.ffmpegPath.browse"),
          t("settings.ffmpegPath.detect"),
          t("settings.ffmpegPath.detected", { version: "" }),
          t("settings.ffmpegPath.invalid", { error: "" }),
          t("settings.ffmpegPath.notDetected"),
        ],
      },
      {
        id: "data-backup",
        title: t("settings.dataBackup"),
        description: t("settings.dataBackupDescription"),
        summary: t("settings.dataBackupSummary"),
        terms: [
          t("settings.dataBackupExport"),
          t("settings.dataBackupValidate"),
          t("settings.dataBackupRestore"),
          t("settings.dataBackupCredentialsNote"),
        ],
      },
      {
        id: "about-updates",
        title: t("settings.aboutUpdates"),
        description: t("settings.aboutUpdatesDescription"),
        summary: updater.currentVersion
          ? t("settings.aboutUpdatesSummary", { version: updater.currentVersion })
          : undefined,
        terms: [
          t("settings.currentVersion"),
          t("settings.autoUpdateCheck"),
          t("settings.autoUpdateCheckDescription"),
          t("settings.checkForUpdates"),
          t("settings.upToDate"),
          t("settings.installAndRestart"),
        ],
      },
    ],
    [
      browserStatus,
      enabledDesktopIntegrations,
      enabledSchedules,
      ffmpegPath,
      ffmpegProbeError,
      ffmpegVersion,
      installedBrowserCount,
      localeLabel,
      maxActiveTasks,
      maxConnectionsPerHost,
      proxyModeLabel,
      segmentCount,
      speedLimitSummary,
      t,
      themeLabel,
      updater.currentVersion,
    ],
  );
  const settingsSectionById = useMemo(
    () => new Map(settingsSections.map((section) => [section.id, section])),
    [settingsSections],
  );
  const settingsSearchHasMatch = settingsSearchHasResults(settingsSections, settingsSearch);
  const settingsSearchActive = settingsSearch.trim().length > 0;

  // ARC-15: recovery action can request focusing the SFTP known-hosts editor.
  useEffect(() => {
    try {
      const focus = sessionStorage.getItem("vibe-settings-focus");
      if (focus === "sftp_known_hosts") {
        sessionStorage.removeItem("vibe-settings-focus");
        setSettingsSearch(t("settings.sftpKnownHosts"));
      }
    } catch {
      // Ignore storage failures.
    }
  }, [t]);

  // Scroll to and briefly highlight the first field matching the search query.
  useEffect(() => {
    if (!settingsSearchActive) return;
    const normalized = settingsSearch.trim().toLowerCase();
    const container = document.querySelector("[data-settings-scroll]");
    if (!container) return;
    const rows = container.querySelectorAll<HTMLElement>("[data-search-key]");
    for (const row of rows) {
      const key = (row.dataset.searchKey ?? "").toLowerCase();
      if (key.includes(normalized)) {
        row.scrollIntoView({ behavior: "smooth", block: "center" });
        row.classList.add("ring-2", "ring-accent-primary", "rounded-md");
        const timeoutId = window.setTimeout(() => {
          row.classList.remove("ring-2", "ring-accent-primary", "rounded-md");
        }, 2000);
        return () => window.clearTimeout(timeoutId);
      }
    }
  }, [settingsSearch, settingsSearchActive]);

  const getSectionProps = (id: SettingsSectionId) => {
    const section = settingsSectionById.get(id);
    if (!section) throw new Error(`Missing settings section: ${id}`);
    return {
      id,
      title: section.title,
      description: section.description,
      summary: section.summary,
      defaultOpen: DEFAULT_EXPANDED_SETTINGS_SECTIONS.has(id),
      matchesSearch: settingsSectionMatchesQuery(section, settingsSearch),
    };
  };

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
          if (entry.isIntersecting && entry.intersectionRatio > (best?.ratio ?? 0)) {
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
    const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
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
    setAccentColor(settings.accentColor);
    setTitlebarGradientEnabled(settings.titlebarGradientEnabled);
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
    setCompletionRunCommand(settings.completionRunCommand ?? "");
    setDeleteToTrash(settings.deleteToTrash);
    setAutoUpdateCheckEnabled(settings.autoUpdateCheckEnabled);
    setFfmpegPath(settings.ffmpegPath ?? "");
    setBtUploadLimitBps(settings.btUploadLimitBps ?? "");
    // Reset detection state when the source setting changes; the user can
    // re-probe by clicking the Detect button.
    setFfmpegVersion(null);
    setFfmpegProbeError(null);
    setSaveState("saved");
  }, [settings]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: saveSettings is a stable store action; form fields define the debounce snapshot.
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
      accentColor === settings.accentColor &&
      titlebarGradientEnabled === settings.titlebarGradientEnabled &&
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
      completionRunCommand === (settings.completionRunCommand ?? "") &&
      deleteToTrash === settings.deleteToTrash &&
      autoUpdateCheckEnabled === settings.autoUpdateCheckEnabled &&
      ffmpegPath === (settings.ffmpegPath ?? "") &&
      btUploadLimitBps === (settings.btUploadLimitBps ?? "") &&
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
        accentColor,
        titlebarGradientEnabled,
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
        completionRunCommand,
        deleteToTrash,
        autoUpdateCheckEnabled,
        ffmpegPath: ffmpegPath.trim() || null,
        btUploadLimitBps: btUploadLimitBps.trim() || null,
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
    accentColor,
    titlebarGradientEnabled,
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
    completionRunCommand,
    deleteToTrash,
    autoUpdateCheckEnabled,
    ffmpegPath,
    btUploadLimitBps,
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
      setError(localizedErrorMessage(err, t));
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
        accentColor: nextSettings.accentColor,
        titlebarGradientEnabled: nextSettings.titlebarGradientEnabled,
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
        completionRunCommand: nextSettings.completionRunCommand ?? "",
        deleteToTrash: nextSettings.deleteToTrash,
        autoUpdateCheckEnabled: nextSettings.autoUpdateCheckEnabled,
        ffmpegPath: nextSettings.ffmpegPath,
        btUploadLimitBps: nextSettings.btUploadLimitBps,
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
        setError(localizedErrorMessage(err, t));
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
        description: localizedErrorMessage(err, t),
      });
    }
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setDefaultSaveDir(selected);
  }

  async function handleBrowseFfmpegPath() {
    const filters = navigator.platform.toLowerCase().includes("win")
      ? [{ name: "ffmpeg", extensions: ["exe"] }]
      : undefined;
    const selected = await openFilePicker(filters);
    if (selected?.path) {
      setFfmpegPath(selected.path);
      // Reset stale detection state when the path changes.
      setFfmpegVersion(null);
      setFfmpegProbeError(null);
    }
  }

  async function handleDetectFfmpeg() {
    setFfmpegProbing(true);
    setFfmpegVersion(null);
    setFfmpegProbeError(null);
    try {
      const trimmed = ffmpegPath.trim();
      const version = await probeFfmpegVersion(trimmed || null);
      setFfmpegVersion(version);
    } catch (err) {
      setFfmpegProbeError(localizedErrorMessage(err, t));
    } finally {
      setFfmpegProbing(false);
    }
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
        accentColor: "blue",
        titlebarGradientEnabled: false,
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
        completionRunCommand: "",
        deleteToTrash: true,
        autoUpdateCheckEnabled: true,
        ffmpegPath: null,
        btUploadLimitBps: null,
      });
      setSettings(updated);
      setDefaultSaveDir(updated.defaultSaveDir);
      setMaxActiveTasks(updated.maxActiveTasks);
      setGlobalSpeedLimitBps(updated.globalSpeedLimitBps ? String(updated.globalSpeedLimitBps) : "");
      setMultiConnectionThresholdBytes(
        updated.multiConnectionThresholdBytes ? String(updated.multiConnectionThresholdBytes) : "",
      );
      setSegmentCount(updated.segmentCount);
      setMaxConnectionsPerHost(updated.maxConnectionsPerHost);
      setSystemNotifications(updated.systemNotifications);
      setCloseToTray(updated.closeToTray);
      setStartOnBoot(updated.startOnBoot);
      setAutoResumeOnStartup(updated.autoResumeOnStartup);
      setFloatingWindowEnabled(updated.floatingWindowEnabled);
      setClipboardMonitorEnabled(updated.clipboardMonitorEnabled);
      setAccentColor(updated.accentColor);
      setTitlebarGradientEnabled(updated.titlebarGradientEnabled);
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
      setScheduleSpeedLimitBps(updated.scheduleSpeedLimitBps ? String(updated.scheduleSpeedLimitBps) : "");
      setCompletionAction(updated.completionAction);
      setCompletionCountdownSeconds(updated.completionCountdownSeconds);
      setCompletionRunCommand(updated.completionRunCommand ?? "");
      setDeleteToTrash(updated.deleteToTrash);
      setAutoUpdateCheckEnabled(updated.autoUpdateCheckEnabled);
      setFfmpegPath(updated.ffmpegPath ?? "");
      setBtUploadLimitBps(updated.btUploadLimitBps ?? "");
      setFfmpegVersion(null);
      setFfmpegProbeError(null);
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
        description: localizedErrorMessage(err, t),
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
      `Native host ready: ${browserStatus.nativeHostReady}`,
      `Native host error: ${browserStatus.nativeHostError ?? "(none)"}`,
      `Capture available: ${browserStatus.captureAvailable}`,
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
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setBrowserExporting(false);
    }
  }

  function patchBrowserCaptureDraft(patch: Partial<BrowserCaptureSettings>) {
    setBrowserCaptureDraft((current) => (current ? mergeBrowserCapturePatch(current, patch) : current));
  }

  // UX-03: persist the latest draft after debounce; never disable fields while saving.
  // biome-ignore lint/correctness/useExhaustiveDependencies: persistBrowserCaptureDraft reads latest draft via closure; epoch guards stale IPC.
  useEffect(() => {
    if (!browserCaptureDraft || !browserCapture) return;
    if (captureSettingsEqual(browserCaptureDraft, browserCapture)) return;
    if (browserCaptureSaveTimer.current) clearTimeout(browserCaptureSaveTimer.current);
    const snapshot = browserCaptureDraft;
    browserCaptureSaveTimer.current = setTimeout(() => {
      void persistBrowserCaptureDraft(snapshot);
    }, BROWSER_CAPTURE_SAVE_DEBOUNCE_MS);
    return () => {
      if (browserCaptureSaveTimer.current) clearTimeout(browserCaptureSaveTimer.current);
    };
  }, [browserCaptureDraft, browserCapture]);

  async function persistBrowserCaptureDraft(snapshot: BrowserCaptureSettings) {
    const epoch = ++browserCaptureSaveEpoch.current;
    setBrowserCaptureSaving(true);
    try {
      const next = await updateBrowserCaptureSettings({
        experimentalCaptureEnabled: snapshot.experimentalCaptureEnabled,
        autoIntercept: snapshot.autoIntercept,
        forwardHeaders: snapshot.forwardHeaders,
        forwardHeadersMode: snapshot.forwardHeadersMode,
        minSizeBytes: snapshot.minSizeBytes,
        fileExtensions: snapshot.fileExtensions,
        siteRules: snapshot.siteRules,
        allowIntranetHandoff: snapshot.allowIntranetHandoff,
      });
      if (epoch !== browserCaptureSaveEpoch.current) return;
      setBrowserCapture(next);
      setBrowserCaptureDraft((draft) => resolveCaptureDraftAfterSave(draft, snapshot, next));
      setBrowserStatus((current) => (current ? { ...current, capture: next } : current));
    } catch (err) {
      if (epoch !== browserCaptureSaveEpoch.current) return;
      log.error("browser capture settings update failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      if (epoch === browserCaptureSaveEpoch.current) {
        setBrowserCaptureSaving(false);
      }
    }
  }

  function resetDirectory() {
    setDefaultSaveDir("");
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: startup loading is attempted once to avoid retry loops after failure.
  useEffect(() => {
    if (!settings && !loading) void refreshSettings();
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the subscription owns refreshes after the initial request.
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
      const capture = status.capture ?? (await getBrowserCaptureSettings());
      setBrowserCapture(capture);
      setBrowserCaptureDraft(capture);
    } catch (err) {
      log.error("browser integration status refresh failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
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
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setBrowserAction(null);
    }
  }

  async function handleExportBackup() {
    if (!isTauriRuntime()) {
      addToast({ tone: "error", title: t("settings.dataBackupUnavailable") });
      return;
    }
    setBackupBusy(true);
    try {
      const { exportAppBackup } = await import("@/lib/backup");
      const result = await exportAppBackup();
      if (!result) return;
      addToast({
        tone: "success",
        title: t("settings.dataBackupExportSuccess", { path: result.path }),
      });
    } catch (err) {
      log.error("backup export failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setBackupBusy(false);
    }
  }

  async function handleValidateBackup() {
    if (!isTauriRuntime()) {
      addToast({ tone: "error", title: t("settings.dataBackupUnavailable") });
      return;
    }
    setBackupBusy(true);
    try {
      const { validateSelectedAppBackup } = await import("@/lib/backup");
      const result = await validateSelectedAppBackup();
      if (!result) return;
      addToast({
        tone: "success",
        title: t("settings.dataBackupValidateSuccess", {
          schema: result.schemaVersion,
          bytes: result.databaseBytes,
        }),
      });
    } catch (err) {
      log.error("backup validate failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setBackupBusy(false);
    }
  }

  async function handleRestoreBackup() {
    if (!isTauriRuntime()) {
      addToast({ tone: "error", title: t("settings.dataBackupUnavailable") });
      return;
    }
    if (!window.confirm(t("settings.dataBackupRestoreConfirm"))) return;
    setBackupBusy(true);
    try {
      const { restoreSelectedAppBackup } = await import("@/lib/backup");
      const result = await restoreSelectedAppBackup();
      if (!result) return;
      addToast({
        tone: "success",
        title: t("settings.dataBackupRestoreSuccess", {
          path: result.preRestoreBackupPath,
        }),
      });
    } catch (err) {
      log.error("backup restore failed", err);
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setBackupBusy(false);
    }
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <SettingsSearchContext.Provider value={{ query: settingsSearch, setQuery: setSettingsSearch }}>
          <div ref={sectionNavRef} data-settings-scroll className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
            <nav
              className="sticky top-0 z-10 border-b border-border-subtle bg-surface-base"
              aria-label={t("settings.sectionsNav")}
            >
              <div className="mx-auto max-w-4xl px-3 pt-2 sm:px-4 md:px-6">
                <div className="relative flex items-center">
                  <Search className="pointer-events-none absolute left-3 h-4 w-4 text-text-muted" />
                  <Input
                    type="search"
                    value={settingsSearch}
                    onChange={(e) => setSettingsSearch(e.target.value)}
                    placeholder={t("settings.searchSettings")}
                    aria-label={t("settings.searchSettings")}
                    className="h-9 w-full rounded-md border border-border-subtle bg-surface-root pl-9 pr-8 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                  />
                  {settingsSearch ? (
                    <button
                      type="button"
                      onClick={() => setSettingsSearch("")}
                      className="absolute right-2 rounded p-1.5 min-h-8 min-w-8 text-text-muted hover:text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                      aria-label={t("settings.clearSearch")}
                    >
                      <X className="h-4 w-4" />
                    </button>
                  ) : null}
                </div>
              </div>
              <div className="mx-auto flex max-w-4xl items-center gap-2 px-3 sm:px-4 md:px-6">
                <div className="min-w-0 flex-1 lg:hidden">
                  <Select value={activeSection} onValueChange={(value) => scrollToSection(value as SettingsSectionId)}>
                    <SelectTrigger className="h-9 w-full bg-surface-root" aria-label={t("settings.sectionsNav")}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {settingsSections.map(({ id, title }) => (
                        <SelectItem key={id} value={id}>
                          {title}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="hidden min-w-0 flex-1 gap-1 overflow-x-auto overflow-y-hidden lg:flex">
                  {settingsSections.map(({ id, title }) => (
                    <button
                      key={id}
                      type="button"
                      onClick={() => scrollToSection(id)}
                      aria-current={activeSection === id ? "page" : undefined}
                      className={cn(
                        "relative shrink-0 px-3 py-2.5 text-[13px] font-medium transition-colors",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary",
                        activeSection === id ? "text-accent-primary" : "text-text-muted hover:text-text-secondary",
                      )}
                    >
                      {title}
                      {activeSection === id ? (
                        <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-accent-primary" />
                      ) : null}
                    </button>
                  ))}
                </div>
                <SaveStatus state={saveState} saving={saving} className="shrink-0" />
              </div>
            </nav>
            <div className="mx-auto flex w-full max-w-4xl flex-col px-3 py-4 sm:px-4 md:px-6 md:py-5">
              {error ? (
                <div
                  className="mb-4 flex flex-wrap items-center gap-2 rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
                  role="alert"
                >
                  <span className="min-w-0 flex-1">{error}</span>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-8 shrink-0 border-border-danger text-status-danger hover:bg-status-danger/10 hover:text-status-danger"
                    onClick={() => void refreshSettings()}
                    disabled={loading}
                  >
                    <RotateCcw className={cn("h-3.5 w-3.5", loading && "animate-spin")} aria-hidden />
                    {t("settings.retryLoad")}
                  </Button>
                </div>
              ) : null}

              {settingsSearchActive && !settingsSearchHasMatch ? (
                <div
                  className="mb-4 rounded-md border border-border-subtle bg-surface-base px-3 py-2 text-sm text-text-secondary"
                  role="status"
                >
                  {t("settings.searchNoResults")}
                </div>
              ) : null}

              <SettingsSection {...getSectionProps("downloads")}>
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
                  searchKey="max_active_tasks"
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
                  searchKey="speed_limit"
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

                <SettingsToggle
                  title={t("settings.deleteToTrash")}
                  description={t("settings.deleteToTrashTip")}
                  checked={deleteToTrash}
                  disabled={controlsDisabled}
                  onChange={setDeleteToTrash}
                />
              </SettingsSection>

              <SettingsSection {...getSectionProps("advanced-downloads")}>
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
                  searchKey="max_connections"
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
                        setMaxConnectionsPerHost(Math.min(MAX_CONNECTIONS_PER_HOST, Math.max(1, Math.floor(next))));
                      }
                    }}
                    disabled={controlsDisabled}
                    className="h-11 w-28 bg-surface-root text-center font-mono md:h-8"
                  />
                </SettingsRow>

                <SettingsRow
                  title={t("settings.btUploadLimit")}
                  htmlFor="bt-upload-limit"
                  tip={t("settings.btUploadLimitTip")}
                  searchKey="bt_upload_limit"
                >
                  <ByteUnitInput
                    id="bt-upload-limit"
                    valueBytes={btUploadLimitBps}
                    onChange={setBtUploadLimitBps}
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

                <ClassificationRulesEditor />
              </SettingsSection>

              <SettingsSection {...getSectionProps("scheduled-downloads")}>
                <SettingsSubHeading title={t("settings.scheduleGroupDownload")} />
                <SettingsToggle
                  title={t("settings.downloadWindow")}
                  description={t("settings.downloadWindowDescription")}
                  checked={scheduleDownloadWindowEnabled}
                  disabled={controlsDisabled}
                  onChange={setScheduleDownloadWindowEnabled}
                />
                <SettingsRow
                  title={t("settings.downloadWindowTime")}
                  controlClassName="max-w-sm"
                  tip={t("settings.downloadWindowTimeTip")}
                >
                  <div className="flex items-center gap-2">
                    <Input
                      type="time"
                      value={scheduleDownloadWindowStart}
                      onChange={(event) => setScheduleDownloadWindowStart(event.target.value)}
                      disabled={controlsDisabled || !scheduleDownloadWindowEnabled}
                      aria-label={t("settings.startTime")}
                      className="h-11 w-32 bg-surface-root font-mono md:h-8"
                    />
                    <span className="text-xs text-text-muted">-</span>
                    <Input
                      type="time"
                      value={scheduleDownloadWindowEnd}
                      onChange={(event) => setScheduleDownloadWindowEnd(event.target.value)}
                      disabled={controlsDisabled || !scheduleDownloadWindowEnabled}
                      aria-label={t("settings.endTime")}
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
                <SettingsRow
                  title={t("settings.speedLimitWindowTime")}
                  controlClassName="max-w-sm"
                  tip={t("settings.speedLimitWindowTimeTip")}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <Input
                      type="time"
                      value={scheduleSpeedLimitWindowStart}
                      onChange={(event) => setScheduleSpeedLimitWindowStart(event.target.value)}
                      disabled={controlsDisabled || !scheduleSpeedLimitWindowEnabled}
                      aria-label={t("settings.startTime")}
                      className="h-11 w-32 bg-surface-root font-mono md:h-8"
                    />
                    <span className="text-xs text-text-muted">-</span>
                    <Input
                      type="time"
                      value={scheduleSpeedLimitWindowEnd}
                      onChange={(event) => setScheduleSpeedLimitWindowEnd(event.target.value)}
                      disabled={controlsDisabled || !scheduleSpeedLimitWindowEnabled}
                      aria-label={t("settings.endTime")}
                      className="h-11 w-32 bg-surface-root font-mono md:h-8"
                    />
                  </div>
                </SettingsRow>
                <SettingsRow
                  title={t("settings.scheduledSpeedLimit")}
                  htmlFor="scheduled-speed-limit"
                  tip={t("settings.scheduledSpeedLimitTip")}
                >
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
                <SettingsRow
                  title={t("settings.completionAction")}
                  htmlFor="completion-action"
                  tip={t("settings.completionActionTip")}
                >
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
                      <SelectItem value="sleep">{t("settings.completionSleep")}</SelectItem>
                      <SelectItem value="hibernate">{t("settings.completionHibernate")}</SelectItem>
                      <SelectItem value="lock_screen">{t("settings.completionLockScreen")}</SelectItem>
                      <SelectItem value="run_command">{t("settings.completionRunCommand")}</SelectItem>
                    </SelectContent>
                  </Select>
                </SettingsRow>
                {completionAction === "run_command" ? (
                  <SettingsRow
                    title={t("settings.completionCommandLabel")}
                    htmlFor="completion-run-command"
                    tip={t("settings.completionCommandTip")}
                  >
                    <Input
                      id="completion-run-command"
                      value={completionRunCommand}
                      onChange={(event) => setCompletionRunCommand(event.target.value)}
                      placeholder={t("settings.completionCommandPlaceholder")}
                      disabled={controlsDisabled}
                      className="h-8 font-mono"
                    />
                  </SettingsRow>
                ) : null}
                <SettingsRow
                  title={t("settings.completionCountdown")}
                  htmlFor="completion-countdown"
                  tip={t("settings.completionCountdownTip")}
                >
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

              <SettingsSection {...getSectionProps("network")}>
                <SettingsRow
                  title={t("settings.proxyMode")}
                  htmlFor="proxy-mode-select"
                  tip={t("settings.proxyModeTip")}
                >
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

                <SettingsRow title={t("settings.proxyUrl")} htmlFor="proxy-url" searchKey="proxy_url">
                  <div className="grid gap-2">
                    <Input
                      id="proxy-url"
                      value={proxyUrl}
                      onChange={(event) => setProxyUrl(event.target.value)}
                      placeholder="socks5://127.0.0.1:1080"
                      disabled={controlsDisabled || proxyMode !== "custom"}
                      className="h-11 bg-surface-root font-mono md:h-8"
                    />
                    <p className="text-xs leading-5 text-text-muted">{t("settings.proxyUrlDescription")}</p>
                  </div>
                </SettingsRow>

                <SettingsRow
                  title={t("settings.proxyNoProxy")}
                  htmlFor="proxy-no-proxy"
                  tip={t("settings.proxyNoProxyTip")}
                >
                  <Input
                    id="proxy-no-proxy"
                    value={proxyNoProxy}
                    onChange={(event) => setProxyNoProxy(event.target.value)}
                    placeholder="localhost,127.0.0.1,.local"
                    disabled={controlsDisabled || proxyMode !== "custom"}
                    className="h-11 bg-surface-root font-mono md:h-8"
                  />
                </SettingsRow>

                <SettingsRow
                  title={t("settings.proxyUsername")}
                  htmlFor="proxy-username"
                  tip={t("settings.proxyAuthTip")}
                >
                  <Input
                    id="proxy-username"
                    value={proxyUsername}
                    onChange={(event) => setProxyUsername(event.target.value)}
                    disabled={controlsDisabled || proxyMode !== "custom"}
                    className="h-11 bg-surface-root md:h-8"
                  />
                </SettingsRow>

                <SettingsRow
                  title={t("settings.proxyPassword")}
                  htmlFor="proxy-password"
                  tip={t("settings.proxyPasswordTip")}
                >
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
                        proxyPasswordSaved ? t("settings.proxyPasswordSaved") : t("settings.proxyPasswordPlaceholder")
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

                <SettingsRow title={t("settings.sftpKnownHosts")} searchKey="sftp_known_hosts">
                  <SftpKnownHostsEditor disabled={controlsDisabled} />
                </SettingsRow>
              </SettingsSection>

              <SettingsSection {...getSectionProps("interface")}>
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
                      {SUPPORTED_LOCALES.map((locale) => (
                        <SelectItem key={locale} value={locale}>
                          <span className="flex items-center gap-2">
                            {LOCALE_LABEL_KEYS[locale] ? t(LOCALE_LABEL_KEYS[locale]) : locale}
                            {!STABLE_LOCALES.includes(locale) && (
                              <span className="rounded bg-surface-hover px-1.5 py-0.5 text-xs text-text-muted">
                                {t("locale.beta")}
                              </span>
                            )}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </SettingsRow>
                <SettingsRow title={t("settings.accentColor")} htmlFor="accent-color-picker">
                  <fieldset
                    id="accent-color-picker"
                    className="m-0 flex min-w-0 flex-wrap items-center gap-2.5 border-0 p-0"
                  >
                    <legend className="sr-only">{t("settings.accentColor")}</legend>
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
                      const isSelected = accentColor === color;
                      const swatchBg = isSelected
                        ? "var(--accent-primary)"
                        : (ACCENT_SWATCHES[color]?.[isDark ? "dark" : "light"] ?? "var(--accent-primary)");
                      return (
                        <button
                          key={color}
                          type="button"
                          aria-pressed={isSelected}
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
                  </fieldset>
                </SettingsRow>
                <SettingsToggle
                  title={t("settings.titlebarGradient")}
                  description={t("settings.titlebarGradientDescription")}
                  checked={titlebarGradientEnabled}
                  disabled={controlsDisabled}
                  onChange={setTitlebarGradientEnabled}
                />
              </SettingsSection>

              <SettingsSection {...getSectionProps("desktop-integration")}>
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

              <SettingsSection {...getSectionProps("browser-integration")}>
                <p className="border-b border-border-divider px-4 py-3 text-xs leading-5 text-text-muted">
                  {t("settings.browserIntegrationTip")}
                </p>
                {browserCaptureDraft ? (
                  <BrowserCaptureControls
                    settings={browserCaptureDraft}
                    available={browserStatus?.captureAvailable ?? false}
                    disabled={browserLoading || !browserStatus?.captureAvailable}
                    saving={browserCaptureSaving}
                    onUpdate={patchBrowserCaptureDraft}
                  />
                ) : null}
                <div className="grid">
                  {browserStatus?.browsers.map((browser) => {
                    const disabled =
                      browserLoading ||
                      browserAction === browser.browser ||
                      !browser.supportedOnPlatform ||
                      !browserStatus.nativeHostReady;
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
                          <p
                            className="mt-1 truncate text-xs text-text-muted"
                            title={browser.manifestPath ?? t("settings.browserNoManifestPath")}
                          >
                            {browser.manifestPath ?? t("settings.browserNoManifestPath")}
                          </p>
                          <p
                            className="mt-1 truncate text-xs text-text-muted"
                            title={`${browser.profile} / ${browser.extensionId ?? ""}`}
                          >
                            {browser.profile} / {browser.extensionId ?? t("settings.browserNoExtensionId")}
                          </p>
                          {browser.lastError ? (
                            <p className="mt-1 text-xs text-status-danger">{browser.lastError}</p>
                          ) : null}
                        </div>
                        <Button
                          type="button"
                          variant={browser.manifestInstalled ? "ghost" : "outline"}
                          className="h-11 justify-center md:h-8"
                          onClick={() => void setBrowserInstalled(browser)}
                          disabled={disabled}
                        >
                          {browser.manifestInstalled ? <Trash2 className="h-4 w-4" /> : <Check className="h-4 w-4" />}
                          {browser.manifestInstalled ? t("settings.browserUninstall") : t("settings.browserInstall")}
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
                    {!browserStatus.nativeHostReady ? (
                      <p className="mb-3 text-status-danger" role="alert">
                        {browserStatus.nativeHostError ?? t("settings.browserHostUnavailable")}
                      </p>
                    ) : null}
                    <p>
                      {t("settings.browserHostName")}{" "}
                      <span className="font-mono text-text-secondary">{browserStatus.nativeHostName}</span>
                    </p>
                    <p className="truncate" title={browserStatus.nativeHostPath ?? ""}>
                      {t("settings.browserNativeHostPath")}{" "}
                      <span className="font-mono text-text-secondary">
                        {browserStatus.nativeHostPath ?? t("settings.browserNoManifestPath")}
                      </span>
                    </p>
                    <p className="truncate" title={browserStatus.extensionCorePath ?? ""}>
                      {t("settings.browserExtensionPath")}{" "}
                      <span className="font-mono text-text-secondary">
                        {browserStatus.extensionCorePath ?? t("settings.browserBuildExtensions")}
                      </span>
                    </p>
                    {browserExportResult ? (
                      <p className="mt-2 truncate" title={browserExportResult.outputDir}>
                        {t("settings.browserExportPath")}{" "}
                        <span className="font-mono text-text-secondary">{browserExportResult.outputDir}</span>
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

              <SettingsSection {...getSectionProps("environment")}>
                <EnvironmentPanel onFocusSection={scrollToSection} />
              </SettingsSection>

              <SettingsSection {...getSectionProps("external-tools")}>
                <SettingsRow
                  title={t("settings.ffmpegPath.label")}
                  htmlFor="ffmpeg-path"
                  tip={t("settings.ffmpegPath.description")}
                  searchKey="ffmpeg"
                >
                  <div className="flex flex-col gap-3">
                    <div className="flex flex-wrap items-center gap-2">
                      <Input
                        id="ffmpeg-path"
                        type="text"
                        value={ffmpegPath}
                        onChange={(e) => setFfmpegPath(e.target.value)}
                        placeholder={t("settings.ffmpegPath.placeholder")}
                        className="h-11 flex-1 font-mono text-sm md:h-8"
                        disabled={controlsDisabled || ffmpegProbing}
                      />
                      <Button
                        type="button"
                        variant="outline"
                        className="h-11 md:h-8"
                        disabled={controlsDisabled || ffmpegProbing}
                        onClick={() => void handleBrowseFfmpegPath()}
                      >
                        <FolderOpen className="h-4 w-4" />
                        {t("settings.ffmpegPath.browse")}
                      </Button>
                      <Button
                        type="button"
                        className="h-11 md:h-8"
                        disabled={controlsDisabled || ffmpegProbing}
                        onClick={() => void handleDetectFfmpeg()}
                      >
                        {ffmpegProbing ? (
                          <LoaderCircle className="h-4 w-4 animate-spin" />
                        ) : (
                          <Sparkles className="h-4 w-4" />
                        )}
                        {ffmpegProbing ? t("settings.ffmpegPath.detecting") : t("settings.ffmpegPath.detect")}
                      </Button>
                    </div>
                    {ffmpegVersion ? (
                      <div className="flex items-center gap-2 text-sm text-text-secondary">
                        <Check className="h-4 w-4 text-status-success" />
                        <span className="font-mono">{ffmpegVersion}</span>
                      </div>
                    ) : ffmpegProbeError ? (
                      <div className="flex items-start gap-2 text-sm text-status-danger">
                        <X className="mt-0.5 h-4 w-4 shrink-0" />
                        <span className="break-all">{ffmpegProbeError}</span>
                      </div>
                    ) : !ffmpegPath.trim() ? (
                      <div className="flex items-center gap-2 text-sm text-text-muted">
                        <Info className="h-4 w-4" />
                        <span>{t("settings.ffmpegPath.notDetected")}</span>
                      </div>
                    ) : null}
                  </div>
                </SettingsRow>
              </SettingsSection>

              <SettingsSection {...getSectionProps("data-backup")}>
                <SettingsRow title={t("settings.dataBackup")} tip={t("settings.dataBackupCredentialsNote")}>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      className="h-11 md:h-8"
                      disabled={controlsDisabled || backupBusy}
                      onClick={() => void handleExportBackup()}
                    >
                      {backupBusy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Archive className="h-4 w-4" />}
                      {t("settings.dataBackupExport")}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      className="h-11 md:h-8"
                      disabled={controlsDisabled || backupBusy}
                      onClick={() => void handleValidateBackup()}
                    >
                      <Check className="h-4 w-4" />
                      {t("settings.dataBackupValidate")}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      className="h-11 md:h-8"
                      disabled={controlsDisabled || backupBusy}
                      onClick={() => void handleRestoreBackup()}
                    >
                      <RotateCcw className="h-4 w-4" />
                      {t("settings.dataBackupRestore")}
                    </Button>
                  </div>
                </SettingsRow>
              </SettingsSection>

              <SettingsSection {...getSectionProps("about-updates")}>
                <SettingsRow title={t("settings.currentVersion")} htmlFor="app-version">
                  <span id="app-version" className="font-mono text-sm text-text-secondary">
                    {updater.currentVersion ?? "—"}
                  </span>
                </SettingsRow>
                <SettingsToggle
                  title={t("settings.autoUpdateCheck")}
                  description={t("settings.autoUpdateCheckDescription")}
                  checked={autoUpdateCheckEnabled}
                  disabled={controlsDisabled}
                  onChange={setAutoUpdateCheckEnabled}
                />
                <div className="border-t border-border-divider px-4 py-4">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                    <div className="min-w-0">
                      <UpdateStatusBadge updater={updater} />
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-2">
                      <Button
                        type="button"
                        variant="outline"
                        className="h-11 md:h-8"
                        disabled={!updater.isTauri || updater.checking || updater.installing}
                        onClick={() => void updater.checkForUpdate()}
                      >
                        {updater.checking ? (
                          <LoaderCircle className="h-4 w-4 animate-spin" />
                        ) : (
                          <RotateCcw className="h-4 w-4" />
                        )}
                        {updater.checking ? t("settings.checkingForUpdates") : t("settings.checkForUpdates")}
                      </Button>
                      {updater.status === "available" ? (
                        <Button
                          type="button"
                          className="h-11 md:h-8"
                          disabled={updater.installing}
                          onClick={() => void updater.installUpdate()}
                        >
                          {updater.installing ? (
                            <LoaderCircle className="h-4 w-4 animate-spin" />
                          ) : (
                            <Sparkles className="h-4 w-4" />
                          )}
                          {t("settings.installAndRestart")}
                        </Button>
                      ) : null}
                      {updater.status === "error" ? (
                        <Button
                          type="button"
                          variant="outline"
                          className="h-11 md:h-8"
                          disabled={updater.checking}
                          onClick={() => void updater.checkForUpdate()}
                        >
                          <RotateCcw className="h-4 w-4" />
                          {t("settings.retryCheck")}
                        </Button>
                      ) : null}
                      {updater.status === "available" && !updater.installing ? (
                        <Button type="button" variant="ghost" className="h-11 md:h-8" onClick={updater.dismissUpdate}>
                          <X className="h-4 w-4" />
                          {t("settings.dismissUpdate")}
                        </Button>
                      ) : null}
                    </div>
                  </div>

                  {updater.status === "downloading" && updater.progress ? (
                    <UpdateProgressBar updater={updater} />
                  ) : null}

                  {updater.status === "available" && updater.releaseNotes ? (
                    <div className="mt-3">
                      <p className="mb-1 text-xs font-medium text-text-secondary">{t("settings.releaseNotes")}</p>
                      <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-md border border-border-subtle bg-surface-root px-3 py-2 text-xs leading-5 text-text-secondary">
                        {updater.releaseNotes}
                      </pre>
                    </div>
                  ) : null}

                  {updater.status === "available" && updater.updateDate ? (
                    <p className="mt-2 text-xs text-text-muted">
                      {t("settings.updateDate")}: {formatReleaseDate(updater.updateDate)}
                    </p>
                  ) : null}
                </div>
              </SettingsSection>
              <div className="flex justify-center pt-6 pb-2">
                <button
                  type="button"
                  onClick={() => setShowResetDialog(true)}
                  className="inline-flex items-center gap-1.5 rounded-md px-3 py-2 text-sm text-text-muted transition-colors hover:text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  {t("settings.resetDefaults")}
                </button>
              </div>
            </div>
          </div>
        </SettingsSearchContext.Provider>

        <Dialog open={showResetDialog} onOpenChange={setShowResetDialog}>
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle>{t("settings.resetDefaultsTitle")}</DialogTitle>
              <DialogDescription className="space-y-2">
                <span className="block">{t("settings.resetDefaultsConfirm")}</span>
                <span className="block text-text-muted">{t("settings.resetDefaultsPreserved")}</span>
              </DialogDescription>
            </DialogHeader>
            <DialogBody />
            <DialogFooter className="gap-2 sm:gap-2">
              <Button variant="outline" onClick={() => setShowResetDialog(false)} disabled={resetting}>
                {t("actions.cancel")}
              </Button>
              <Button variant="danger" onClick={handleResetDefaults} disabled={resetting}>
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
  const label = isSaving ? t("settings.saving") : state === "saved" ? t("settings.saved") : t("settings.autoSave");

  return (
    <div
      className={cn("inline-flex min-h-8 shrink-0 items-center gap-1.5 px-1 text-xs text-text-muted", className)}
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
  summary,
  defaultOpen = false,
  matchesSearch = true,
  children,
}: {
  id?: string;
  title: string;
  description?: string;
  summary?: string;
  defaultOpen?: boolean;
  matchesSearch?: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const { query, setQuery } = useSettingsSearch();
  const searchActive = query.trim().length > 0;
  const [open, setOpen] = useState(defaultOpen);
  const contentVisible = searchActive ? matchesSearch : open;
  const panelId = id ? `${id}-settings-section-panel` : undefined;

  return (
    <section id={id} className="scroll-mt-28 border-t border-border-subtle py-4 first:border-t-0 first:pt-0">
      <div className="flex w-full items-start justify-between gap-3 rounded-md px-1 py-2">
        <h2 className="grid min-w-0 flex-1 gap-1 text-sm font-semibold text-text-primary">
          <span className="flex min-w-0 flex-wrap items-center gap-2">
            <span>{title}</span>
            {summary ? (
              <span className="min-w-0 truncate rounded-full bg-surface-raised px-2 py-0.5 text-[11px] font-medium text-text-muted">
                {summary}
              </span>
            ) : null}
          </span>
          {description ? (
            <span className="max-w-2xl text-sm font-medium leading-5 text-text-muted">{description}</span>
          ) : null}
        </h2>
        <button
          type="button"
          className={cn(
            "mt-0.5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-text-muted transition-colors",
            "hover:bg-surface-base/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
          )}
          aria-expanded={contentVisible}
          aria-controls={panelId}
          aria-label={
            contentVisible ? t("settings.collapseSection", { title }) : t("settings.expandSection", { title })
          }
          onClick={() => {
            if (searchActive) return;
            setOpen((current) => !current);
          }}
        >
          <ChevronDown
            className={cn("h-4 w-4 shrink-0 transition-transform duration-ui", contentVisible && "rotate-180")}
            aria-hidden
          />
        </button>
      </div>
      {contentVisible ? (
        <SettingsSearchContext.Provider
          value={{
            query,
            setQuery,
            forceVisible: searchActive && matchesSearch,
          }}
        >
          <div id={panelId} className="mt-2 overflow-hidden rounded-lg border border-border-panel bg-surface-base/70">
            {children}
          </div>
        </SettingsSearchContext.Provider>
      ) : null}
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
  const exact = [...units].reverse().find(([unit]) => bytes >= Number(unit) && bytes % Number(unit) === 0);
  return exact?.[0] ?? units[0]?.[0] ?? "1";
}

function displayAmount(valueBytes: string, unitSize: number, allowEmpty?: boolean): string {
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
  const { query, forceVisible } = useSettingsSearch();
  const fieldId = useId();
  const titleId = `${fieldId}-title`;
  const descriptionId = `${fieldId}-description`;
  const matchesSearch =
    forceVisible ||
    !query ||
    title.toLowerCase().includes(query.toLowerCase()) ||
    description.toLowerCase().includes(query.toLowerCase());
  if (!matchesSearch) return null;

  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <span className="min-w-0">
        <span id={titleId} className="block text-sm font-medium text-text-primary">
          {title}
        </span>
        <span id={descriptionId} className="mt-1 block text-xs leading-5 text-text-muted">
          {description}
        </span>
      </span>
      <Switch
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
      />
    </div>
  );
}

function SettingsRow({
  title,
  children,
  htmlFor,
  controlClassName,
  tip,
  searchKey,
}: {
  title: string;
  children: ReactNode;
  htmlFor?: string;
  controlClassName?: string;
  tip?: string;
  searchKey?: string;
}) {
  const { query, forceVisible } = useSettingsSearch();
  const matchesSearch =
    forceVisible ||
    !query ||
    title.toLowerCase().includes(query.toLowerCase()) ||
    (tip ?? "").toLowerCase().includes(query.toLowerCase()) ||
    (searchKey ?? "").toLowerCase().includes(query.toLowerCase());
  if (!matchesSearch) return null;
  return (
    <div
      data-search-key={searchKey}
      className="grid gap-3 border-t border-border-divider px-4 py-4 first:border-t-0 md:grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] md:items-center"
    >
      <div className="flex items-center gap-1.5">
        <label className="text-sm font-medium text-text-secondary" htmlFor={htmlFor}>
          {title}
        </label>
        {tip ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="inline-flex min-h-8 min-w-8 shrink-0 items-center justify-center rounded-full text-text-muted transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/50"
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
      <h3 className="text-xs font-medium text-text-secondary">{title}</h3>
    </div>
  );
}

type UpdaterSnapshot = ReturnType<typeof useAppUpdater>;

function UpdateStatusBadge({ updater }: { updater: UpdaterSnapshot }) {
  const { t } = useTranslation();

  if (updater.status === "checking") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("settings.checkingForUpdates")}
      </span>
    );
  }
  if (updater.status === "up-to-date") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-status-success">
        <Check className="h-4 w-4" />
        {t("settings.upToDate")}
      </span>
    );
  }
  if (updater.status === "available") {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <span className="inline-flex items-center gap-2 text-sm font-medium text-accent-primary">
          <Sparkles className="h-4 w-4" />
          {t("settings.updateAvailableTitle", { version: updater.updateVersion ?? "" })}
        </span>
        <span className="text-xs text-text-muted">{t("settings.updateAvailableDescription")}</span>
      </span>
    );
  }
  if (updater.status === "downloading") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("settings.downloadingUpdate")}
      </span>
    );
  }
  if (updater.status === "installing") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("settings.installingUpdate")}
      </span>
    );
  }
  if (updater.status === "error") {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <span className="inline-flex items-center gap-2 text-sm text-status-danger">
          <X className="h-4 w-4" />
          {t("settings.updateCheckFailed")}
        </span>
        {updater.error ? (
          <span className="truncate text-xs text-text-muted" title={updater.error}>
            {updater.error}
          </span>
        ) : null}
      </span>
    );
  }
  return <span className="text-sm text-text-muted">{t("settings.checkForUpdates")}</span>;
}

function UpdateProgressBar({ updater }: { updater: UpdaterSnapshot }) {
  const { t } = useTranslation();
  const progress = updater.progress;
  if (!progress) return null;

  const downloaded = progress.downloadedBytes;
  const total = progress.totalBytes;
  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;

  const label =
    total && total > 0
      ? t("settings.updateProgress", {
          downloaded: formatBytes(downloaded),
          total: formatBytes(total),
        })
      : t("settings.updateProgressUnknown", { downloaded: formatBytes(downloaded) });

  return (
    <div className="mt-3 flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-xs text-text-muted">
        <span>{label}</span>
        {percent !== null ? <span className="font-mono tabular-nums">{percent}%</span> : null}
      </div>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full bg-surface-root"
        role="progressbar"
        aria-label={label}
        aria-valuenow={percent ?? 0}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className="h-full rounded-full bg-accent-primary transition-[width] duration-ui"
          style={{ width: `${percent ?? 100}%` }}
        />
      </div>
    </div>
  );
}

function formatReleaseDate(isoDate: string): string {
  try {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return isoDate;
    return date.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return isoDate;
  }
}
