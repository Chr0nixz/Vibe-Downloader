import { lazy, Suspense, useEffect } from "react";

import { AppShell } from "@/components/shell/AppShell";
import { StartupGate } from "@/components/shell/StartupGate";

const FloatingStatusWindow = lazy(() =>
  import("@/components/shell/FloatingStatusWindow").then((module) => ({
    default: module.FloatingStatusWindow,
  })),
);

const TrayMenu = lazy(() =>
  import("@/components/shell/TrayMenu").then((module) => ({
    default: module.TrayMenu,
  })),
);

export default function App() {
  const surface = typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("surface");

  useEffect(() => {
    if (surface) {
      document.documentElement.dataset.surface = surface;
    } else {
      delete document.documentElement.dataset.surface;
    }

    return () => {
      delete document.documentElement.dataset.surface;
    };
  }, [surface]);

  if (surface === "tray-menu") {
    return (
      <Suspense fallback={null}>
        <TrayMenu />
      </Suspense>
    );
  }

  if (surface === "floating-status") {
    return (
      <Suspense fallback={null}>
        <FloatingStatusWindow />
      </Suspense>
    );
  }

  return (
    <StartupGate>
      <AppShell />
    </StartupGate>
  );
}
