import type { AppSettings } from "@/generated/bindings";
import { updateSettings } from "@/lib/tauri";

export async function applyGlobalSpeedLimit(
  settings: AppSettings,
  limit: number | null,
): Promise<AppSettings> {
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
    floatingWindowEnabled: settings.floatingWindowEnabled,
    fontFamily: settings.fontFamily,
  });
}
