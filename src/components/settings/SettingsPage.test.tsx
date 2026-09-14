import { act, fireEvent, render, screen } from "@testing-library/react";
import { axe } from "jest-axe";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { AppSettings, BrowserCaptureSettings, BrowserIntegrationStatus } from "@/generated/bindings";
import { openDirectoryPicker, openFilePicker } from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";
import { SettingsPage } from "./SettingsPage";

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  getBrowserIntegrationStatus: vi.fn(),
  onBrowserIntegrationChanged: vi.fn(),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { language: "en" },
    }),
  };
});

vi.mock("next-themes", () => ({
  useTheme: () => ({ theme: "system", resolvedTheme: "light", setTheme: vi.fn() }),
}));

vi.mock("@/hooks/use-app-updater", () => ({
  useAppUpdater: () => ({
    currentVersion: "0.2.0",
    updateVersion: null,
    releaseNotes: null,
    updateDate: null,
    status: "idle",
    progress: null,
    error: null,
    checking: false,
    installing: false,
    isTauri: false,
    checkForUpdate: vi.fn(),
    installUpdate: vi.fn(),
    dismissUpdate: vi.fn(),
  }),
}));

vi.mock("@/components/settings/BrowserCaptureControls", () => ({
  BrowserCaptureControls: () => null,
}));

vi.mock("@/components/settings/ClassificationRulesEditor", () => ({
  ClassificationRulesEditor: () => null,
}));

vi.mock("@/lib/tauri", () => ({
  exportBrowserExtensionPackages: vi.fn(),
  getBrowserCaptureSettings: vi.fn(),
  getBrowserIntegrationStatus: mocks.getBrowserIntegrationStatus,
  getSettings: vi.fn(),
  installBrowserIntegration: vi.fn(),
  isTauriRuntime: () => false,
  onBrowserIntegrationChanged: mocks.onBrowserIntegrationChanged,
  openDirectoryPicker: vi.fn(),
  openFilePicker: vi.fn(),
  probeFfmpegVersion: vi.fn(),
  uninstallBrowserIntegration: vi.fn(),
  updateBrowserCaptureSettings: vi.fn(),
  updateSettings: mocks.updateSettings,
}));

const SETTINGS: AppSettings = {
  maxActiveTasks: 2,
  defaultSaveDir: "D:\\Downloads",
  globalSpeedLimitBps: null,
  multiConnectionThresholdBytes: String(16 * 1024 * 1024),
  segmentCount: 4,
  maxConnectionsPerHost: 8,
  systemNotifications: true,
  closeToTray: false,
  startOnBoot: false,
  autoResumeOnStartup: false,
  floatingWindowEnabled: false,
  clipboardMonitorEnabled: true,
  accentColor: "blue",
  proxyMode: "off",
  proxyUrl: "",
  proxyNoProxy: "",
  proxyUsername: "",
  proxyPasswordSaved: false,
  scheduleDownloadWindowEnabled: false,
  scheduleDownloadWindowStart: "00:00",
  scheduleDownloadWindowEnd: "06:00",
  scheduleSpeedLimitWindowEnabled: false,
  scheduleSpeedLimitWindowStart: "18:00",
  scheduleSpeedLimitWindowEnd: "23:00",
  scheduleSpeedLimitBps: null,
  titlebarGradientEnabled: false,
  completionAction: "none",
  completionCountdownSeconds: 30,
  completionRunCommand: "",
  deleteToTrash: true,
  autoUpdateCheckEnabled: true,
  ffmpegPath: null,
  btUploadLimitBps: null,
};

const CAPTURE: BrowserCaptureSettings = {
  experimentalCaptureEnabled: false,
  autoIntercept: false,
  forwardHeaders: false,
  forwardHeadersMode: "disabled",
  minSizeBytes: "0",
  fileExtensions: [],
  siteRules: [],
  allowIntranetHandoff: false,
};

const BROWSER_STATUS: BrowserIntegrationStatus = {
  nativeHostName: "com.vibe_downloader.native_host",
  nativeHostPath: null,
  nativeHostReady: false,
  nativeHostError: "not bundled in test",
  extensionCorePath: null,
  captureAvailable: false,
  experimentalCaptureEnabled: false,
  realtime: { wsUrl: null, connected: false },
  capture: CAPTURE,
  browsers: [],
};

class MockIntersectionObserver {
  readonly root = null;
  readonly rootMargin = "0px";
  readonly thresholds: number[] = [];

  disconnect() {
    return undefined;
  }

  observe(_target: Element) {
    return undefined;
  }

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  unobserve(_target: Element) {
    return undefined;
  }
}

function renderSettings() {
  return render(
    <TooltipProvider>
      <SettingsPage />
    </TooltipProvider>,
  );
}

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("SettingsPage", () => {
  beforeEach(() => {
    vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
    mocks.updateSettings.mockReset();
    mocks.getBrowserIntegrationStatus.mockReset();
    mocks.onBrowserIntegrationChanged.mockReset();
    mocks.updateSettings.mockImplementation(async (input: Partial<AppSettings>) => ({ ...SETTINGS, ...input }));
    mocks.getBrowserIntegrationStatus.mockResolvedValue(BROWSER_STATUS);
    mocks.onBrowserIntegrationChanged.mockResolvedValue(() => {});
    vi.stubGlobal(
      "matchMedia",
      vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    );
    useSettingsStore.setState({ settings: SETTINGS, loading: false, error: null });
  });

  afterEach(() => {
    if (vi.isFakeTimers()) {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
    vi.unstubAllGlobals();
  });

  it("debounces rapid edits and saves only the latest settings snapshot", async () => {
    vi.useFakeTimers();
    renderSettings();
    await act(async () => {});

    const maxActiveTasks = screen.getByLabelText("settings.maxActiveTasks");
    fireEvent.change(maxActiveTasks, { target: { value: "3" } });
    act(() => vi.advanceTimersByTime(500));
    fireEvent.change(maxActiveTasks, { target: { value: "4" } });
    act(() => vi.advanceTimersByTime(999));
    expect(mocks.updateSettings).not.toHaveBeenCalled();

    await act(async () => vi.advanceTimersByTime(1));

    expect(mocks.updateSettings).toHaveBeenCalledTimes(1);
    expect(mocks.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ maxActiveTasks: 4 }));
  });

  it("associates a clamped numeric setting with a programmatic range hint (UX-18)", () => {
    renderSettings();
    const input = screen.getByLabelText("settings.maxActiveTasks");
    fireEvent.change(input, { target: { value: "99" } });

    const hint = screen.getByText("settings.valueClampedToRange");
    expect(hint).toHaveAttribute("id", "max-active-tasks-range-hint");
    expect(input).toHaveAttribute("aria-describedby", "max-active-tasks-range-hint");
    expect(input).toHaveValue(8);

    fireEvent.change(input, { target: { value: "5" } });
    expect(screen.queryByText("settings.valueClampedToRange")).not.toBeInTheDocument();
    expect(input).not.toHaveAttribute("aria-describedby");
  });

  it("cancels a pending save when the settings page unmounts", async () => {
    vi.useFakeTimers();
    const view = renderSettings();
    await act(async () => {});

    fireEvent.change(screen.getByLabelText("settings.maxActiveTasks"), { target: { value: "3" } });
    view.unmount();
    await act(async () => vi.advanceTimersByTime(1000));

    expect(mocks.updateSettings).not.toHaveBeenCalled();
  });

  it("surfaces the latest autosave failure and leaves the saving state", async () => {
    vi.useFakeTimers();
    mocks.updateSettings.mockRejectedValueOnce(new Error("permission denied"));
    renderSettings();
    await act(async () => {});

    fireEvent.change(screen.getByLabelText("settings.maxActiveTasks"), { target: { value: "3" } });
    await act(async () => vi.advanceTimersByTime(1000));

    expect(screen.getByRole("alert")).toHaveTextContent("permission denied");
    expect(screen.getByText("settings.autoSave")).toBeInTheDocument();
    expect(useSettingsStore.getState().settings?.maxActiveTasks).toBe(2);
  });

  it("ignores an older autosave response that resolves after the latest save", async () => {
    vi.useFakeTimers();
    const firstSave = createDeferred<AppSettings>();
    const latestSave = createDeferred<AppSettings>();
    mocks.updateSettings
      .mockImplementationOnce(() => firstSave.promise)
      .mockImplementationOnce(() => latestSave.promise);
    renderSettings();
    await act(async () => {});

    const maxActiveTasks = screen.getByLabelText<HTMLInputElement>("settings.maxActiveTasks");
    fireEvent.change(maxActiveTasks, { target: { value: "3" } });
    await act(async () => vi.advanceTimersByTime(1000));
    fireEvent.change(maxActiveTasks, { target: { value: "4" } });
    await act(async () => vi.advanceTimersByTime(1000));

    expect(mocks.updateSettings).toHaveBeenCalledTimes(2);
    await act(async () => {
      latestSave.resolve({ ...SETTINGS, maxActiveTasks: 4 });
      await latestSave.promise;
    });
    expect(useSettingsStore.getState().settings?.maxActiveTasks).toBe(4);
    expect(screen.getByText("settings.saved")).toBeInTheDocument();

    await act(async () => {
      firstSave.resolve({ ...SETTINGS, maxActiveTasks: 3 });
      await firstSave.promise;
    });
    expect(useSettingsStore.getState().settings?.maxActiveTasks).toBe(4);
    expect(maxActiveTasks).toHaveValue(4);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("exposes the compact section navigator and labeled save state", async () => {
    renderSettings();
    await act(async () => {});

    expect(screen.getByRole("combobox", { name: "settings.sectionsNav" })).toBeInTheDocument();
    expect(screen.getByText("settings.autoSave")).toHaveAttribute("aria-live", "polite");
  });

  it("jumps to a later settings section by scrolling only the settings container", async () => {
    renderSettings();
    await act(async () => {});

    const container = document.querySelector("[data-settings-scroll]");
    expect(container).toBeInstanceOf(HTMLElement);
    const scrollTo = vi.fn();
    (container as HTMLElement).scrollTo = scrollTo;

    fireEvent.click(screen.getByRole("button", { name: "settings.aboutUpdates", hidden: true }));

    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: expect.any(Number) }));
  });

  it("has no automated accessibility violations in the default settings surface", async () => {
    const view = renderSettings();
    await act(async () => {});

    const results = await axe(view.container);
    expect(results.violations).toEqual([]);
  });

  // UX-25: a rejecting picker previously died as a silent unhandled rejection.
  it("shows an error toast when the save-dir picker rejects", async () => {
    vi.mocked(openDirectoryPicker).mockRejectedValueOnce(new Error("picker down"));
    useToastStore.setState({ toasts: [] });

    renderSettings();
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "settings.chooseDirectory", hidden: true }));
    await act(async () => {});

    expect(
      useToastStore.getState().toasts.some((toast) => toast.tone === "error" && toast.title === "toast.actionFailed"),
    ).toBe(true);
  });

  it("shows an error toast when the ffmpeg path picker rejects", async () => {
    vi.mocked(openFilePicker).mockRejectedValueOnce(new Error("picker down"));
    useToastStore.setState({ toasts: [] });

    renderSettings();
    await act(async () => {});
    // External tools is collapsed by default; find its expand trigger by the
    // panel id it controls, then reach the browse button.
    const expandTrigger = screen
      .getAllByRole("button", { hidden: true })
      .find((button) => button.getAttribute("aria-controls") === "external-tools-settings-section-panel");
    expect(expandTrigger).toBeDefined();
    fireEvent.click(expandTrigger!);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "settings.ffmpegPath.browse", hidden: true }));
    await act(async () => {});

    expect(
      useToastStore.getState().toasts.some((toast) => toast.tone === "error" && toast.title === "toast.actionFailed"),
    ).toBe(true);
  });
});
