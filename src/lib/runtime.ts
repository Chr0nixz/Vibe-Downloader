/** True when the UI runs inside a Tauri webview (not Vite dev in a plain browser). */
export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  return "__TAURI_INTERNALS__" in window || "__TAURI__" in window;
}
