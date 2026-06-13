import { useEffect, useState } from "react";

export type ShellLayout = "narrow" | "medium" | "wide";
const RESIZE_DEBOUNCE_MS = 80;

export function readShellLayout(width = typeof window !== "undefined" ? window.innerWidth : 1280): ShellLayout {
  if (width < 768) return "narrow";
  if (width < 1024) return "medium";
  return "wide";
}

export function useShellLayout(): ShellLayout {
  const [layout, setLayout] = useState<ShellLayout>(() => readShellLayout());

  useEffect(() => {
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const update = () => {
      setLayout((current) => {
        const next = readShellLayout(window.innerWidth);
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

  return layout;
}

export function useIsCompactShell(): boolean {
  const layout = useShellLayout();
  return layout !== "wide";
}
