import type { AppSettings } from "@/generated/bindings";
import { updateSettings } from "@/lib/tauri";

export async function applyGlobalSpeedLimit(settings: AppSettings, limit: number | null): Promise<AppSettings> {
  return updateSettings({
    maxActiveTasks: settings.maxActiveTasks,
    defaultSaveDir: settings.defaultSaveDir,
    globalSpeedLimitBps: limit && limit > 0 ? String(limit) : null,
    multiConnectionThresholdBytes: settings.multiConnectionThresholdBytes,
    segmentCount: settings.segmentCount,
    maxConnectionsPerHost: settings.maxConnectionsPerHost,
    systemNotifications: settings.systemNotifications,
    closeToTray: settings.closeToTray,
    startOnBoot: settings.startOnBoot,
    autoResumeOnStartup: settings.autoResumeOnStartup,
    floatingWindowEnabled: settings.floatingWindowEnabled,
    clipboardMonitorEnabled: settings.clipboardMonitorEnabled,
    accentColor: settings.accentColor,
    titlebarGradientEnabled: settings.titlebarGradientEnabled,
    proxyMode: settings.proxyMode,
    proxyUrl: settings.proxyUrl,
    proxyNoProxy: settings.proxyNoProxy,
    proxyUsername: settings.proxyUsername,
    proxyPassword: null,
    clearProxyPassword: false,
    scheduleDownloadWindowEnabled: settings.scheduleDownloadWindowEnabled,
    scheduleDownloadWindowStart: settings.scheduleDownloadWindowStart,
    scheduleDownloadWindowEnd: settings.scheduleDownloadWindowEnd,
    scheduleSpeedLimitWindowEnabled: settings.scheduleSpeedLimitWindowEnabled,
    scheduleSpeedLimitWindowStart: settings.scheduleSpeedLimitWindowStart,
    scheduleSpeedLimitWindowEnd: settings.scheduleSpeedLimitWindowEnd,
    scheduleSpeedLimitBps: settings.scheduleSpeedLimitBps,
    completionAction: settings.completionAction,
    completionCountdownSeconds: settings.completionCountdownSeconds,
    completionRunCommand: settings.completionRunCommand,
    deleteToTrash: settings.deleteToTrash,
  });
}
