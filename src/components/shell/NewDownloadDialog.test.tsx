import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { ProbePhasePayload, ProbeTaskPayload } from "@/generated/bindings";
import { useSettingsStore } from "@/stores/settings-store";
import type { Task } from "@/types/task";
import { NewDownloadDialog } from "./NewDownloadDialog";

const mocks = vi.hoisted(() => ({
  createTask: vi.fn(),
  importUrls: vi.fn(),
  onProbePhase: vi.fn(),
  openDirectoryPicker: vi.fn(),
  openFilePicker: vi.fn(),
  probeFtpDirectory: vi.fn(),
  probeSftpDirectory: vi.fn(),
  probeTask: vi.fn(),
  probeWebdavDirectory: vi.fn(),
  phaseHandler: undefined as ((payload: ProbePhasePayload) => void) | undefined,
  unlisten: vi.fn(),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
    }),
  };
});

vi.mock("@/lib/local-file", () => ({
  getLocalFileKind: () => "text",
  pathToFileUrl: (path: string) => `file://${path}`,
  readFileAsText: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  createTask: mocks.createTask,
  importUrls: mocks.importUrls,
  onProbePhase: mocks.onProbePhase,
  openDirectoryPicker: mocks.openDirectoryPicker,
  openFilePicker: mocks.openFilePicker,
  probeFtpDirectory: mocks.probeFtpDirectory,
  probeSftpDirectory: mocks.probeSftpDirectory,
  probeTask: mocks.probeTask,
  probeWebdavDirectory: mocks.probeWebdavDirectory,
}));

function makeProbe(url: string, fileName: string): ProbeTaskPayload {
  return {
    inputUrl: url,
    finalUrl: url,
    fileName,
    protocol: "https",
    taskKind: "single_file",
    capabilities: {
      supportsResume: true,
      supportsParallel: true,
      supportsMultiFile: false,
    },
    files: [{ relativePath: fileName, size: "1024", contentType: "application/octet-stream" }],
    totalSize: "1024",
    sourceKey: "example.com",
    contentType: "application/octet-stream",
    etag: '"probe-etag"',
    lastModified: null,
    hlsVariants: [],
    hlsAudioTracks: [],
    hlsSubtitleTracks: [],
    probedAt: "2026-07-14T00:00:00.000Z",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

function renderDialog() {
  const onCreated = vi.fn();
  const onOpenChange = vi.fn();
  const view = render(
    <TooltipProvider>
      <NewDownloadDialog open onOpenChange={onOpenChange} onCreated={onCreated} />
    </TooltipProvider>,
  );
  return { ...view, onCreated, onOpenChange };
}

async function startAutomaticProbe(url: string) {
  fireEvent.change(screen.getByLabelText("newDownload.url"), { target: { value: url } });
  await act(async () => vi.advanceTimersByTime(650));
}

describe("NewDownloadDialog probe flow", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.phaseHandler = undefined;
    mocks.onProbePhase.mockImplementation(async (handler: (payload: ProbePhasePayload) => void) => {
      mocks.phaseHandler = handler;
      return mocks.unlisten;
    });
    useSettingsStore.setState({ settings: null, loading: false, error: null });
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("probes after the debounce and submits the matching snapshot", async () => {
    const url = "https://example.com/release.zip";
    const probe = makeProbe(url, "release.zip");
    const created = { id: "created-task" } as Task;
    mocks.probeTask.mockResolvedValue(probe);
    mocks.createTask.mockResolvedValue(created);
    const { onCreated, onOpenChange } = renderDialog();

    await startAutomaticProbe(url);

    expect(mocks.probeTask).toHaveBeenCalledWith(expect.objectContaining({ url, requestId: "1" }));
    expect(screen.getByText("release.zip")).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "newDownload.start" }));
    });

    expect(mocks.createTask).toHaveBeenCalledWith(expect.objectContaining({ url, probeSnapshot: probe }));
    expect(onCreated).toHaveBeenCalledWith(created);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("shows a structured timeout error and leaves the URL editable", async () => {
    const encodedError = JSON.stringify({
      code: "timeout",
      message: "Probe timed out",
      recoverable: true,
      actions: ["retry"],
    });
    mocks.probeTask.mockRejectedValue(encodedError);
    renderDialog();

    await startAutomaticProbe("https://slow.example.com/archive.zip");

    expect(screen.getByRole("alert")).toHaveTextContent("Probe timed out");
    expect(screen.getByRole("alert")).toHaveTextContent("newDownload.probeErrorTimeout");
    expect(screen.getByLabelText("newDownload.url")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("newDownload.url")).not.toBeDisabled();
  });

  it("ignores stale probe responses after the URL changes", async () => {
    const firstUrl = "https://example.com/old.zip";
    const secondUrl = "https://example.com/new.zip";
    const first = deferred<ProbeTaskPayload>();
    const second = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    renderDialog();

    await startAutomaticProbe(firstUrl);
    fireEvent.change(screen.getByLabelText("newDownload.url"), { target: { value: secondUrl } });
    await act(async () => vi.advanceTimersByTime(650));

    await act(async () => second.resolve(makeProbe(secondUrl, "new.zip")));
    expect(screen.getByText("new.zip")).toBeInTheDocument();

    await act(async () => first.resolve(makeProbe(firstUrl, "old.zip")));
    expect(screen.queryByText("old.zip")).not.toBeInTheDocument();
    expect(screen.getByText("new.zip")).toBeInTheDocument();
  });

  it("accepts only probe-phase events for the active request", async () => {
    const pending = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockReturnValue(pending.promise);
    renderDialog();
    await act(async () => {});

    await startAutomaticProbe("https://example.com/phase.zip");
    expect(screen.getByText("newDownload.probePhaseConnecting")).toBeInTheDocument();

    act(() => mocks.phaseHandler?.({ requestId: "stale", kind: "checking_ffmpeg", protocol: "hls" }));
    expect(screen.queryByText("newDownload.probePhaseCheckingFfmpeg")).not.toBeInTheDocument();

    act(() => mocks.phaseHandler?.({ requestId: "1", kind: "querying_metadata", protocol: "https" }));
    expect(screen.getByText("newDownload.probePhaseQueryingMetadata")).toBeInTheDocument();

    await act(async () => pending.resolve(makeProbe("https://example.com/phase.zip", "phase.zip")));
  });

  it("unsubscribes from probe-phase events when unmounted", async () => {
    const view = renderDialog();
    await act(async () => {});

    view.unmount();

    expect(mocks.unlisten).toHaveBeenCalledTimes(1);
  });
});
