import type { AppSettings } from "@/generated/bindings";
import { updateSettings } from "@/lib/tauri";

export async function applyGlobalSpeedLimit(limit: number | null): Promise<AppSettings> {
  return updateSettings({ globalSpeedLimitBps: limit && limit > 0 ? String(limit) : null });
}
