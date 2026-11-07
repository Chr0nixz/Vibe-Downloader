import { useEffect } from "react";

/**
 * Marks `<html data-window-inactive>` while the app window is not the one the
 * user is working in (another app has focus, or the window is hidden), so CSS
 * can stop decorative motion that would otherwise run in peripheral vision.
 */
export function useWindowActivityAttribute(): void {
  useEffect(() => {
    const root = document.documentElement;
    const sync = () => root.toggleAttribute("data-window-inactive", document.hidden || !document.hasFocus());
    sync();
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
      root.removeAttribute("data-window-inactive");
    };
  }, []);
}
