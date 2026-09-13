import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StartupStatus } from "@/generated/bindings";

import { StartupGate } from "./StartupGate";

const getStartupStatus = vi.fn<() => Promise<StartupStatus>>();
const retryStartupInit = vi.fn<() => Promise<void>>();
const openStartupLogFolder = vi.fn<() => Promise<void>>();
const openStartupDataFolder = vi.fn<() => Promise<void>>();
const relaunch = vi.fn<() => Promise<void>>();

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, values?: Record<string, string | number>) =>
        values ? `${key} ${Object.values(values).join(" ")}` : key,
      i18n: { language: "en" },
    }),
  };
});

vi.mock("motion/react", () => ({
  useReducedMotion: () => true,
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: () => relaunch(),
}));

vi.mock("@/lib/tauri", () => ({
  getStartupStatus: () => getStartupStatus(),
  retryStartupInit: () => retryStartupInit(),
  openStartupLogFolder: () => openStartupLogFolder(),
  openStartupDataFolder: () => openStartupDataFolder(),
  openDatabaseRecoveryFolder: vi.fn(),
  resetDatabaseForRecovery: vi.fn(),
}));

function failedStatus(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    mode: "startup_failed",
    reason: "database",
    message: "could not open db",
    code: "database",
    databasePath: null,
    backupPath: null,
    backupVerified: false,
    canReset: false,
    logPath: "C:\\logs",
    dataPath: "C:\\data",
    ...overrides,
  };
}

describe("StartupGate", () => {
  beforeEach(() => {
    getStartupStatus.mockReset();
    retryStartupInit.mockReset();
    openStartupLogFolder.mockReset();
    openStartupDataFolder.mockReset();
    relaunch.mockReset();
  });

  it("mounts children when startup becomes ready", async () => {
    getStartupStatus
      .mockResolvedValueOnce({
        mode: "initializing",
        reason: null,
        message: null,
        code: null,
        databasePath: null,
        backupPath: null,
        backupVerified: false,
        canReset: false,
        logPath: null,
        dataPath: null,
      })
      .mockResolvedValue({
        mode: "ready",
        reason: null,
        message: null,
        code: null,
        databasePath: null,
        backupPath: null,
        backupVerified: false,
        canReset: false,
        logPath: null,
        dataPath: null,
      });

    render(
      <StartupGate>
        <p>App ready</p>
      </StartupGate>,
    );

    await waitFor(() => expect(screen.getByText("App ready")).toBeInTheDocument());
  });

  it("shows the startup failed page with diagnostics", async () => {
    getStartupStatus.mockResolvedValue(failedStatus());

    render(
      <StartupGate>
        <p>App ready</p>
      </StartupGate>,
    );

    await waitFor(() => expect(screen.getByRole("heading", { name: "startupFailed.title" })).toBeInTheDocument());
    expect(screen.getByText("could not open db")).toBeInTheDocument();
    expect(screen.getByText("database")).toBeInTheDocument();
    expect(screen.queryByText("App ready")).not.toBeInTheDocument();
  });

  it("retries init and resumes polling until ready", async () => {
    getStartupStatus
      .mockResolvedValueOnce(failedStatus())
      .mockResolvedValueOnce({
        mode: "initializing",
        reason: null,
        message: null,
        code: null,
        databasePath: null,
        backupPath: null,
        backupVerified: false,
        canReset: false,
        logPath: null,
        dataPath: null,
      })
      .mockResolvedValue({
        mode: "ready",
        reason: null,
        message: null,
        code: null,
        databasePath: null,
        backupPath: null,
        backupVerified: false,
        canReset: false,
        logPath: null,
        dataPath: null,
      });
    retryStartupInit.mockResolvedValue();

    render(
      <StartupGate>
        <p>App ready</p>
      </StartupGate>,
    );

    await waitFor(() => expect(screen.getByRole("button", { name: /startupFailed.retry/ })).toBeInTheDocument());
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /startupFailed.retry/ }));
    });

    expect(retryStartupInit).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByText("App ready")).toBeInTheDocument());
  });

  it("recovers from persistent poll errors via the manual Retry button (UX-22)", async () => {
    vi.useFakeTimers();
    try {
      getStartupStatus.mockRejectedValue(new Error("ipc timeout"));

      render(
        <StartupGate>
          <p>App ready</p>
        </StartupGate>,
      );

      // Exhaust the auto-retry budget: initial failure + 3 backoff retries
      // (1s, 2s, 4s) must surface the manual recovery page, not die silently.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000 + 2_000 + 4_000);
      });
      expect(getStartupStatus).toHaveBeenCalledTimes(4);
      expect(screen.getByText("Error: ipc timeout")).toBeInTheDocument();

      // Manual Retry resets the budget and auto-polling picks up the
      // recovered backend without further user input.
      getStartupStatus.mockResolvedValue({
        mode: "ready",
        reason: null,
        message: null,
        code: null,
        databasePath: null,
        backupPath: null,
        backupVerified: false,
        canReset: false,
        logPath: null,
        dataPath: null,
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /startupFailed.retry/ }));
        await vi.advanceTimersByTimeAsync(300);
      });
      expect(screen.getByText("App ready")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps Retry available after opening logs or data folders", async () => {
    getStartupStatus.mockResolvedValue(failedStatus());
    openStartupLogFolder.mockResolvedValue();
    openStartupDataFolder.mockResolvedValue();

    render(
      <StartupGate>
        <p>App ready</p>
      </StartupGate>,
    );

    await waitFor(() => expect(screen.getByRole("button", { name: /startupFailed.retry/ })).toBeInTheDocument());
    const retry = () => screen.getByRole("button", { name: /startupFailed.retry/ });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /startupFailed.openLogs/ }));
    });
    expect(openStartupLogFolder).toHaveBeenCalledTimes(1);
    expect(retry()).not.toBeDisabled();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /startupFailed.openData/ }));
    });
    expect(openStartupDataFolder).toHaveBeenCalledTimes(1);
    expect(retry()).not.toBeDisabled();
  });

  it("shows static initializing copy when reduced motion is preferred", async () => {
    getStartupStatus.mockResolvedValue({
      mode: "initializing",
      reason: null,
      message: null,
      code: null,
      databasePath: null,
      backupPath: null,
      backupVerified: false,
      canReset: false,
      logPath: null,
      dataPath: null,
    });

    render(
      <StartupGate>
        <p>App ready</p>
      </StartupGate>,
    );

    await waitFor(() => expect(screen.getByText("startup.initializing")).toBeInTheDocument());
    const logo = screen.getByRole("status").querySelector("img");
    expect(logo).toBeTruthy();
    expect(logo?.getAttribute("style") ?? "").not.toContain("animation:");
  });

  it("auto-recovers from transient poll errors via bounded backoff (UX-22)", async () => {
    vi.useFakeTimers();
    try {
      getStartupStatus
        .mockRejectedValueOnce(new Error("ipc hiccup 1"))
        .mockRejectedValueOnce(new Error("ipc hiccup 2"))
        .mockResolvedValue({
          mode: "ready",
          reason: null,
          message: null,
          code: null,
          databasePath: null,
          backupPath: null,
          backupVerified: false,
          canReset: false,
          logPath: null,
          dataPath: null,
        });

      render(
        <StartupGate>
          <p>App ready</p>
        </StartupGate>,
      );

      // Initial poll fails → 1s backoff → retry fails → 2s backoff → ready.
      // No manual recovery page may appear during the backoff window.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000 + 2_000);
      });
      expect(getStartupStatus).toHaveBeenCalledTimes(3);
      expect(screen.getByText("App ready")).toBeInTheDocument();
      expect(screen.queryByText(/startupFailed/)).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});
