import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { create } from "zustand";

import { createLogger } from "@/lib/logger";
import { isTauriRuntime } from "@/lib/runtime";
import { getAppVersion } from "@/lib/tauri";

const log = createLogger("updater");

const UPDATE_CHECK_DELAY_MS = 3000;
const MIN_CHECK_INTERVAL_MS = 30_000;

export type UpdateStatus = "idle" | "checking" | "up-to-date" | "available" | "downloading" | "installing" | "error";

export interface UpdateProgress {
  totalBytes?: number;
  downloadedBytes: number;
}

interface UpdaterStore {
  currentVersion: string | null;
  updateVersion: string | null;
  releaseNotes: string | null;
  updateDate: string | null;
  status: UpdateStatus;
  progress: UpdateProgress | null;
  error: string | null;
  lastCheckAt: number;
  initialized: boolean;
  checkForUpdate: (opts?: { silent?: boolean }) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissUpdate: () => void;
  /** Reads the app version and schedules an auto-check if enabled. Safe to call
   * multiple times; only runs once unless `force` is set. */
  init: (autoCheckEnabled: boolean) => void;
}

let autoCheckTimer: ReturnType<typeof setTimeout> | null = null;

export const useUpdaterStore = create<UpdaterStore>((set, get) => ({
  currentVersion: null,
  updateVersion: null,
  releaseNotes: null,
  updateDate: null,
  status: "idle",
  progress: null,
  error: null,
  lastCheckAt: 0,
  initialized: false,

  init: (autoCheckEnabled: boolean) => {
    if (get().initialized) {
      // Already initialized; only re-evaluate the auto-check schedule.
      scheduleAutoCheck(autoCheckEnabled, get().checkForUpdate);
      return;
    }
    set({ initialized: true });

    if (isTauriRuntime()) {
      void getAppVersion()
        .then((version) => set({ currentVersion: version }))
        .catch((err) => log.warn("failed to read app version", err));
    }

    scheduleAutoCheck(autoCheckEnabled, get().checkForUpdate);
  },

  checkForUpdate: async (opts?: { silent?: boolean }) => {
    if (!isTauriRuntime() || import.meta.env.DEV) return;
    const silent = opts?.silent ?? false;

    const now = Date.now();
    if (silent && now - get().lastCheckAt < MIN_CHECK_INTERVAL_MS) return;

    set({ status: "checking", error: null, lastCheckAt: now });

    try {
      const update = await check();
      if (update) {
        set({
          updateVersion: update.version,
          releaseNotes: update.body ?? null,
          updateDate: update.date ?? null,
          status: "available",
        });
      } else {
        set({
          updateVersion: null,
          releaseNotes: null,
          updateDate: null,
          status: "up-to-date",
        });
      }
    } catch (err) {
      log.warn("update check failed", err);
      if (!silent) {
        set({
          error: err instanceof Error ? err.message : String(err),
          status: "error",
        });
      } else {
        set({ status: "idle" });
      }
    }
  },

  installUpdate: async () => {
    if (!isTauriRuntime() || import.meta.env.DEV) return;

    set({ status: "downloading", error: null, progress: { downloadedBytes: 0 } });

    try {
      const update = await check();
      if (!update) {
        set({
          updateVersion: null,
          status: "up-to-date",
          progress: null,
        });
        return;
      }

      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          set({
            progress: {
              totalBytes: event.data.contentLength,
              downloadedBytes: 0,
            },
          });
        } else if (event.event === "Progress") {
          const prev = get().progress;
          set({
            progress: {
              totalBytes: prev?.totalBytes,
              downloadedBytes: (prev?.downloadedBytes ?? 0) + event.data.chunkLength,
            },
          });
        } else if (event.event === "Finished") {
          set({ status: "installing" });
        }
      });

      set({ status: "installing" });
      await relaunch();
    } catch (err) {
      log.error("update install failed", err);
      set({
        error: err instanceof Error ? err.message : String(err),
        status: "error",
        progress: null,
      });
    }
  },

  dismissUpdate: () => {
    set({
      updateVersion: null,
      releaseNotes: null,
      updateDate: null,
      status: "idle",
      error: null,
    });
  },
}));

function scheduleAutoCheck(autoCheckEnabled: boolean, checkFn: (opts?: { silent?: boolean }) => Promise<void>) {
  if (autoCheckTimer) {
    clearTimeout(autoCheckTimer);
    autoCheckTimer = null;
  }
  if (!isTauriRuntime() || import.meta.env.DEV || !autoCheckEnabled) return;

  autoCheckTimer = setTimeout(() => {
    void checkFn({ silent: true });
  }, UPDATE_CHECK_DELAY_MS);
}
