import { useEffect } from "react";

import { AppShell } from "@/components/shell/AppShell";
import { FloatingStatusWindow } from "@/components/shell/FloatingStatusWindow";
import { TrayMenu } from "@/components/shell/TrayMenu";

export default function App() {
  const surface =
    typeof window === "undefined"
      ? null
      : new URLSearchParams(window.location.search).get("surface");

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
    return <TrayMenu />;
  }

  if (surface === "floating-status") {
    return <FloatingStatusWindow />;
  }

  return <AppShell />;
}
