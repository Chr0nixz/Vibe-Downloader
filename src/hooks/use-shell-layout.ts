import { useEffect, useState } from "react";

export type ShellLayout = "narrow" | "medium" | "wide";
export type ChromeLayout = "merged" | "stacked";
const RESIZE_DEBOUNCE_MS = 80;

/** Matches the `md` breakpoint in globals.css (35rem). Below it the shell is a
 * phone-sized window: bottom navigation and 44px touch targets. A desktop
 * window snapped to half a 1366–1920px screen (≈680–960px) stays above it and
 * keeps the dense desktop list; it used to fall under a 768px cutoff and got
 * the phone layout with a mouse attached. */
export const NARROW_MAX_WIDTH = 560;

export function readShellLayout(width = typeof window !== "undefined" ? window.innerWidth : 1280): ShellLayout {
  if (width < NARROW_MAX_WIDTH) return "narrow";
  // Keep the task list readable on laptop-sized windows. A 320px details
  // column plus the expanded rail leaves too little room around 1024px, so
  // use the overlay drawer until the content area can sustain both panes.
  if (width < 1200) return "medium";
  return "wide";
}

/**
 * Whether the command bar rides inside the titlebar. In a snapped or short
 * window the separate 36px titlebar (logo and name only) plus a ~49px command
 * row spent ~85px on chrome; merged, the list gets that height back. Phone
 * widths keep their own row because 44px touch targets do not fit a 36px bar.
 */
export function readChromeLayout(
  width = typeof window !== "undefined" ? window.innerWidth : 1280,
  height = typeof window !== "undefined" ? window.innerHeight : 800,
): ChromeLayout {
  if (width < NARROW_MAX_WIDTH) return "stacked";
  return width < 1024 || height < 600 ? "merged" : "stacked";
}

function useViewportValue<T>(read: () => T): T {
  const [value, setValue] = useState<T>(read);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `read` is a module-level pure reader; re-subscribing when a caller passes a fresh closure would only churn listeners.
  useEffect(() => {
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const update = () => {
      setValue((current) => {
        const next = read();
        return current === next ? current : next;
      });
    };
    const scheduleUpdate = () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(update, RESIZE_DEBOUNCE_MS);
    };

    update();
    window.addEventListener("resize", scheduleUpdate);
    return () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      window.removeEventListener("resize", scheduleUpdate);
    };
  }, []);

  return value;
}

export function useShellLayout(): ShellLayout {
  return useViewportValue(() => readShellLayout());
}

export function useChromeLayout(): ChromeLayout {
  return useViewportValue(() => readChromeLayout());
}

export function useIsCompactShell(): boolean {
  const layout = useShellLayout();
  return layout !== "wide";
}
