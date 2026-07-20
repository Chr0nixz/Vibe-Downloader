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

  it("recovers from a transient status IPC error via Retry", async () => {
    getStartupStatus.mockRejectedValueOnce(new Error("ipc timeout")).mockResolvedValue({
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

    await waitFor(() => expect(screen.getByText("Error: ipc timeout")).toBeInTheDocument());
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /startupFailed.retry/ }));
    });
    // Transient IPC errors retry polling without calling backend retry_startup_init.
    expect(retryStartupInit).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText("App ready")).toBeInTheDocument());
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
});
