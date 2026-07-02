import { useEffect } from "react";

import { isTauriRuntime } from "@/lib/runtime";
import { useSettingsStore } from "@/stores/settings-store";
import { useUpdaterStore } from "@/stores/updater-store";

/**
 * Thin React wrapper around the shared updater Zustand store. Both the
 * StatusBar badge and the Settings "About & Updates" section use this so they
 * observe the same update state. The first mount initializes version reading
 * and schedules the auto-check (when enabled); subsequent mounts are no-ops.
 */
export function useAppUpdater() {
  const store = useUpdaterStore();
  const settings = useSettingsStore((s) => s.settings);
  const autoCheckEnabled = settings?.autoUpdateCheckEnabled ?? true;

  useEffect(() => {
    store.init(autoCheckEnabled);
  }, [store, autoCheckEnabled]);

  const installing = store.status === "downloading" || store.status === "installing";
  const checking = store.status === "checking";

  return {
    currentVersion: store.currentVersion,
    updateVersion: store.updateVersion,
    releaseNotes: store.releaseNotes,
    updateDate: store.updateDate,
    status: store.status,
    progress: store.progress,
    error: store.error,
    checking,
    installing,
    isTauri: isTauriRuntime(),
    checkForUpdate: store.checkForUpdate,
    installUpdate: store.installUpdate,
    dismissUpdate: store.dismissUpdate,
  };
}
