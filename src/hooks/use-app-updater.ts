import { useEffect } from "react";

import { isTauriRuntime } from "@/lib/runtime";
import { useSettingsStore } from "@/stores/settings-store";
import { useUpdaterStore } from "@/stores/updater-store";

/**
 * Thin React wrapper around the shared updater Zustand store. Both the
 * StatusBar badge and the Settings "About & Updates" section use this so they
 * observe the same update state. The first mount initializes version reading
 * and schedules the auto-check (when enabled); subsequent mounts are no-ops.
 *
 * PERF-14: fields are subscribed individually and `init` is invoked through
 * `getState()`, so updater status changes re-render consumers without
 * re-running the init effect — a whole-store subscription used to re-schedule
 * the auto-check timer on every unrelated updater state change.
 */
export function useAppUpdater() {
  const settings = useSettingsStore((s) => s.settings);
  const autoCheckEnabled = settings?.autoUpdateCheckEnabled ?? true;

  // Only the enabled flag belongs in the deps: re-init on toggle is the
  // documented re-schedule path, while updater state changes must not
  // re-run init.
  useEffect(() => {
    useUpdaterStore.getState().init(autoCheckEnabled);
  }, [autoCheckEnabled]);

  const status = useUpdaterStore((s) => s.status);
  const currentVersion = useUpdaterStore((s) => s.currentVersion);
  const updateVersion = useUpdaterStore((s) => s.updateVersion);
  const releaseNotes = useUpdaterStore((s) => s.releaseNotes);
  const updateDate = useUpdaterStore((s) => s.updateDate);
  const progress = useUpdaterStore((s) => s.progress);
  const error = useUpdaterStore((s) => s.error);
  const checkForUpdate = useUpdaterStore((s) => s.checkForUpdate);
  const installUpdate = useUpdaterStore((s) => s.installUpdate);
  const dismissUpdate = useUpdaterStore((s) => s.dismissUpdate);

  const installing = status === "downloading" || status === "installing";
  const checking = status === "checking";

  return {
    currentVersion,
    updateVersion,
    releaseNotes,
    updateDate,
    status,
    progress,
    error,
    checking,
    installing,
    isTauri: isTauriRuntime(),
    checkForUpdate,
    installUpdate,
    dismissUpdate,
  };
}
