import { type } from "@tauri-apps/plugin-os";

export type Platform = "windows" | "macos" | "linux" | "unknown";

function detectPlatformFallback(): Platform {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent.toLowerCase();
  const platform = navigator.platform?.toLowerCase() ?? "";
  if (platform.includes("win") || ua.includes("windows")) return "windows";
  if (platform.includes("mac") || ua.includes("macintosh")) return "macos";
  if (platform.includes("linux") || ua.includes("linux")) return "linux";
  return "unknown";
}

export async function getPlatform(): Promise<Platform> {
  try {
    const osType = await type();
    if (osType === "windows") return "windows";
    if (osType === "macos") return "macos";
    if (osType === "linux") return "linux";
    return detectPlatformFallback();
  } catch {
    return detectPlatformFallback();
  }
}

export function trafficLightsInsetPx(platform: Platform): number {
  return platform === "macos" ? 78 : 0;
}

export function usesCustomTitleBar(platform: Platform): boolean {
  return platform === "windows";
}

export function usesSystemDecorations(platform: Platform): boolean {
  return platform === "linux";
}
