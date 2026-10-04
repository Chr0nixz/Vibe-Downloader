import { createLogger } from "@/lib/logger";
import { isTauriRuntime } from "@/lib/runtime";
import { useSettingsStore } from "@/stores/settings-store";

const log = createLogger("system-notification");

/** Send an OS notification when enabled. Permission prompts are opt-in so a
 * background event never opens a native permission dialog unexpectedly. */
export async function sendSystemNotification(
  title: string,
  body: string,
  options: { requestPermission?: boolean } = {},
): Promise<void> {
  if (!isTauriRuntime()) return;
  if (!useSettingsStore.getState().settings?.systemNotifications) return;

  try {
    const { isPermissionGranted, requestPermission, sendNotification } = await import(
      "@tauri-apps/plugin-notification"
    );
    let granted = await isPermissionGranted();
    if (!granted && options.requestPermission !== false) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    if (!granted) return;
    await sendNotification({ title, body });
  } catch (error) {
    log.warn("system notification failed", error);
  }
}
