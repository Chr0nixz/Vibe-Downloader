import { useCallback, useEffect, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

import { isTauriRuntime } from "@/lib/runtime";

const UPDATE_CHECK_DELAY_MS = 3000;

export function useAppUpdater() {
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime() || import.meta.env.DEV) return;

    const timer = window.setTimeout(async () => {
      try {
        const update = await check();
        if (update) {
          setUpdateVersion(update.version);
        }
      } catch (err) {
        console.warn("Update check failed:", err);
      }
    }, UPDATE_CHECK_DELAY_MS);

    return () => window.clearTimeout(timer);
  }, []);

  const installUpdate = useCallback(async () => {
    if (!isTauriRuntime() || import.meta.env.DEV) return;

    setInstalling(true);
    setError(null);

    try {
      const update = await check();
      if (!update) {
        setUpdateVersion(null);
        setInstalling(false);
        return;
      }

      await update.downloadAndInstall();
      await relaunch();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setInstalling(false);
    }
  }, []);

  return { updateVersion, installing, error, installUpdate };
}
