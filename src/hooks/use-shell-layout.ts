import { useEffect, useState } from "react";

export type ShellLayout = "narrow" | "medium" | "wide";

export function readShellLayout(width = typeof window !== "undefined" ? window.innerWidth : 1280): ShellLayout {
  if (width < 768) return "narrow";
  if (width < 1024) return "medium";
  return "wide";
}

export function useShellLayout(): ShellLayout {
  const [layout, setLayout] = useState<ShellLayout>(() => readShellLayout());

  useEffect(() => {
    const update = () => setLayout(readShellLayout(window.innerWidth));

    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  return layout;
}

export function useIsCompactShell(): boolean {
  const layout = useShellLayout();
  return layout !== "wide";
}
