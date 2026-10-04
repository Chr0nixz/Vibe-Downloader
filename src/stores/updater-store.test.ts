import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useUpdaterStore } from "@/stores/updater-store";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  download: vi.fn(),
  install: vi.fn(),
  close: vi.fn(),
  relaunch: vi.fn(),
  prepare: vi.fn(),
  cancelPrepared: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: () => mocks.check(),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: () => mocks.relaunch(),
}));

vi.mock("@/lib/runtime", () => ({
  isTauriRuntime: () => true,
}));

vi.mock("@/lib/tauri", () => ({
  cancelPreparedAppRelaunch: () => mocks.cancelPrepared(),
  getAppVersion: vi.fn(),
  prepareAppRelaunch: () => mocks.prepare(),
}));

function makeUpdate() {
  return {
    version: "0.6.0",
    download: mocks.download,
    install: mocks.install,
    close: mocks.close,
  };
}

describe("updater shutdown ordering", () => {
  beforeEach(() => {
    vi.stubEnv("DEV", false);
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.check.mockResolvedValue(makeUpdate());
    mocks.download.mockImplementation(async (onEvent: (event: { event: string; data: object }) => void) => {
      onEvent({ event: "Started", data: { contentLength: 4 } });
      onEvent({ event: "Progress", data: { chunkLength: 4 } });
      onEvent({ event: "Finished", data: {} });
    });
    mocks.install.mockResolvedValue(undefined);
    mocks.close.mockResolvedValue(undefined);
    mocks.prepare.mockResolvedValue(undefined);
    mocks.cancelPrepared.mockResolvedValue(undefined);
    mocks.relaunch.mockResolvedValue(undefined);
    useUpdaterStore.setState({
      currentVersion: "0.5.0",
      updateVersion: "0.6.0",
      releaseNotes: null,
      updateDate: null,
      status: "available",
      progress: null,
      error: null,
      lastCheckAt: 0,
      initialized: true,
    });
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("downloads and verifies before draining, then installs and relaunches", async () => {
    const calls: string[] = [];
    mocks.check.mockImplementation(async () => {
      calls.push("check");
      return makeUpdate();
    });
    mocks.download.mockImplementation(async () => {
      calls.push("download");
    });
    mocks.prepare.mockImplementation(async () => {
      calls.push("prepare");
    });
    mocks.install.mockImplementation(async () => {
      calls.push("install");
    });
    mocks.relaunch.mockImplementation(async () => {
      calls.push("relaunch");
    });
    mocks.close.mockImplementation(async () => {
      calls.push("close");
    });

    await useUpdaterStore.getState().installUpdate();

    expect(calls).toEqual(["check", "download", "prepare", "install", "relaunch", "close"]);
  });

  it("does not start draining or install when package download is cancelled", async () => {
    mocks.download.mockRejectedValue(new Error("download cancelled"));

    await useUpdaterStore.getState().installUpdate();

    expect(mocks.prepare).not.toHaveBeenCalled();
    expect(mocks.install).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();
    expect(mocks.cancelPrepared).not.toHaveBeenCalled();
    expect(mocks.close).toHaveBeenCalledOnce();
    expect(useUpdaterStore.getState().status).toBe("error");
  });

  it("does not install if owners fail to drain and releases the prepared state after install failure", async () => {
    mocks.prepare.mockRejectedValueOnce(new Error("task_stop_pending"));
    await useUpdaterStore.getState().installUpdate();
    expect(mocks.install).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();

    mocks.prepare.mockResolvedValueOnce(undefined);
    mocks.install.mockRejectedValueOnce(new Error("installer failed"));
    await useUpdaterStore.getState().installUpdate();

    expect(mocks.cancelPrepared).toHaveBeenCalledOnce();
    expect(mocks.relaunch).not.toHaveBeenCalled();
    expect(useUpdaterStore.getState().status).toBe("error");
  });
});
