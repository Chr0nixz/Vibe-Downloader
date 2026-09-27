import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { BatchImportResult, ProbePhasePayload, ProbeTaskPayload } from "@/generated/bindings";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";
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
  writeExportFile: vi.fn(),
  save: vi.fn(),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, params?: { line?: number }) => (params?.line ? `${key} ${params.line}` : key),
    }),
  };
});

vi.mock("@/lib/local-file", () => ({
  getLocalFileKind: () => "text",
  pathToFileUrl: (path: string) => `file://${path}`,
  readFileAsText: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  writeExportFile: mocks.writeExportFile,
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

vi.mock("@/lib/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: mocks.save }));

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

    expect(mocks.probeTask).toHaveBeenCalledWith(expect.objectContaining({ url, requestId: expect.any(String) }));
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

    expect(screen.getByRole("alert")).toHaveTextContent("errors.timeout");
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

  it("ignores a probe that resolves after the URL is cleared", async () => {
    const pending = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockReturnValue(pending.promise);
    renderDialog();

    await startAutomaticProbe("https://example.com/old.zip");
    fireEvent.change(screen.getByLabelText("newDownload.url"), { target: { value: "" } });

    await act(async () => pending.resolve(makeProbe("https://example.com/old.zip", "old.zip")));

    expect(screen.queryByText("old.zip")).not.toBeInTheDocument();
    expect(screen.getByLabelText("newDownload.url")).toHaveValue("");
  });

  it("refreshes automatic names while preserving a manually edited name", async () => {
    const first = deferred<ProbeTaskPayload>();
    const second = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    renderDialog();

    await startAutomaticProbe("https://example.com/old.zip");
    await act(async () => first.resolve(makeProbe("https://example.com/old.zip", "old.zip")));
    expect(screen.getByText("old.zip")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("newDownload.url"), {
      target: { value: "https://example.com/new.zip" },
    });
    await act(async () => vi.advanceTimersByTime(650));
    await act(async () => second.resolve(makeProbe("https://example.com/new.zip", "new.zip")));
    expect(screen.getByText("new.zip")).toBeInTheDocument();

    fireEvent.click(screen.getByTitle("newDownload.editFileName"));
    fireEvent.change(screen.getByLabelText("newDownload.fileName"), { target: { value: "keep-name.bin" } });
    fireEvent.change(screen.getByLabelText("newDownload.url"), {
      target: { value: "https://example.com/final.zip" },
    });
    const finalProbe = makeProbe("https://example.com/final.zip", "final.zip");
    mocks.probeTask.mockResolvedValueOnce(finalProbe);
    await act(async () => vi.advanceTimersByTime(650));

    expect(screen.getByDisplayValue("keep-name.bin")).toBeInTheDocument();
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

    act(() =>
      mocks.phaseHandler?.({
        requestId: mocks.probeTask.mock.calls[mocks.probeTask.mock.calls.length - 1]?.[0].requestId,
        kind: "querying_metadata",
        protocol: "https",
      }),
    );
    expect(screen.getByText("newDownload.probePhaseQueryingMetadata")).toBeInTheDocument();

    await act(async () => pending.resolve(makeProbe("https://example.com/phase.zip", "phase.zip")));
  });

  it("rejects responses during debounce, re-probes changed credentials, and ignores closed sessions", async () => {
    const pending = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockReturnValueOnce(pending.promise);
    const view = renderDialog();
    await startAutomaticProbe("https://example.com/a.zip");
    fireEvent.change(screen.getByLabelText("newDownload.url"), { target: { value: "https://example.com/b.zip" } });
    fireEvent.change(screen.getByLabelText("newDownload.url"), { target: { value: "https://example.com/c.zip" } });
    await act(async () => pending.resolve(makeProbe("https://example.com/a.zip", "stale.zip")));
    expect(screen.queryByText("stale.zip")).not.toBeInTheDocument();
    const next = deferred<ProbeTaskPayload>();
    mocks.probeTask.mockReturnValueOnce(next.promise);
    await act(async () => vi.advanceTimersByTime(650));
    view.rerender(
      <TooltipProvider>
        <NewDownloadDialog open={false} onOpenChange={view.onOpenChange} onCreated={view.onCreated} />
      </TooltipProvider>,
    );
    await act(async () => next.resolve(makeProbe("https://example.com/c.zip", "closed.zip")));
    view.rerender(
      <TooltipProvider>
        <NewDownloadDialog open onOpenChange={view.onOpenChange} onCreated={view.onCreated} />
      </TooltipProvider>,
    );
    expect(screen.queryByText("closed.zip")).not.toBeInTheDocument();
  });

  it("retains 100 results with editable failures at 6, 50 and 100 across later previews", async () => {
    const items = Array.from({ length: 100 }, (_, index) => {
      const line = index + 1;
      const failed = [6, 50, 100].includes(line);
      const url = `https://example.com/${line}.zip`;
      return {
        inputUrl: url,
        normalizedUrl: url,
        duplicate: false,
        valid: !failed,
        fileName: `${line}.zip`,
        totalSize: "1",
        contentType: null,
        supportsResume: true,
        errorMessage: failed ? `failed-${line}` : null,
        task: failed ? null : { id: `task-${line}`, url, fileName: `${line}.zip` },
      };
    });
    const result = { items, createdCount: 97, failedCount: 3, duplicateCount: 0 } as BatchImportResult;
    mocks.importUrls.mockResolvedValueOnce(result);
    mocks.save.mockResolvedValue("C:/report.json");
    mocks.writeExportFile.mockResolvedValue(undefined);
    const copy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), {
      target: { value: items.map((item) => item.inputUrl).join("\n") },
    });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "newDownload.createBatch" })));
    for (const line of [6, 50, 100]) {
      expect(screen.getByText(`failed-${line}`)).toBeInTheDocument();
      expect(screen.getByText(`failed-${line}`).closest("details")).toBeNull();
      fireEvent.change(screen.getByLabelText(`newDownload.batchEditUrl ${line}`), {
        target: { value: `https://example.com/fixed-${line}.zip` },
      });
    }
    expect(screen.getByLabelText("newDownload.batchUrls")).toHaveValue(
      [6, 50, 100].map((line) => `https://example.com/${line}.zip`).join("\n"),
    );
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "newDownload.batchCopyFailedUrls" })));
    expect(copy).toHaveBeenCalledWith([6, 50, 100].map((line) => `https://example.com/fixed-${line}.zip`).join("\n"));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "taskList.exportJson" })));
    const exported = JSON.parse(mocks.writeExportFile.mock.calls[0][1]);
    expect(exported).toHaveLength(100);
    expect(exported[99]).toMatchObject({ line: 100, status: "failed", url: "https://example.com/fixed-100.zip" });
    expect(exported[0]).not.toHaveProperty("task");
    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), {
      target: { value: "https://example.com/new.zip" },
    });
    mocks.importUrls.mockResolvedValueOnce({ items: [], createdCount: 0, failedCount: 0, duplicateCount: 0 });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "newDownload.previewBatch" })));
    expect(screen.getByText("failed-100")).toBeInTheDocument();
    for (const line of [6, 50, 100]) {
      mocks.importUrls.mockResolvedValueOnce({
        items: [{ ...items[line - 1], valid: true, errorMessage: null, task: { id: `retry-${line}` } }],
        createdCount: 1,
        failedCount: 0,
        duplicateCount: 0,
      });
      await act(async () => fireEvent.click(screen.getByRole("button", { name: `newDownload.batchRetryOne ${line}` })));
      expect(mocks.importUrls.mock.calls[mocks.importUrls.mock.calls.length - 1]?.[0].input).toBe(
        `https://example.com/fixed-${line}.zip`,
      );
    }
    expect(screen.queryByRole("button", { name: "newDownload.batchRetryFailed" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("newDownload.batchUrls")).toHaveValue("https://example.com/new.zip");
  });

  it("new previews supersede old reads, but cannot release a creation lock or erase its result", async () => {
    const old = deferred<BatchImportResult>();
    const newer = deferred<BatchImportResult>();
    const creating = deferred<BatchImportResult>();
    mocks.importUrls
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(newer.promise)
      .mockReturnValueOnce(creating.promise);
    const view = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    const input = screen.getByLabelText("newDownload.batchUrls");
    fireEvent.change(input, { target: { value: "https://example.com/old" } });
    fireEvent.click(screen.getByRole("button", { name: "newDownload.previewBatch" }));
    fireEvent.change(input, { target: { value: "https://example.com/newer" } });
    fireEvent.click(screen.getByRole("button", { name: "newDownload.previewBatch" }));
    const empty = { items: [], createdCount: 0, failedCount: 0, duplicateCount: 0 };
    await act(async () => newer.resolve(empty));
    act(() => {
      const create = screen.getByRole("button", { name: "newDownload.createBatch" });
      fireEvent.click(create);
      fireEvent.click(create);
    });
    expect(mocks.importUrls).toHaveBeenCalledTimes(3);
    await act(async () => old.resolve(empty));
    expect(screen.getByRole("button", { name: "newDownload.createBatch" })).toBeDisabled();
    fireEvent.change(input, { target: { value: "https://example.com/future" } });
    await act(async () =>
      creating.resolve({
        ...empty,
        createdCount: 1,
        items: [
          { inputUrl: "https://example.com/newer", fileName: "created.zip", task: { id: "created" }, valid: true },
        ],
      } as BatchImportResult),
    );
    expect(view.onCreated).toHaveBeenCalledOnce();
    expect(screen.getByText("created.zip")).toBeInTheDocument();
    expect(input).toHaveValue("https://example.com/future");
    expect(screen.getByRole("button", { name: "newDownload.createBatch" })).not.toBeDisabled();
  });

  it("unsubscribes from probe-phase events when unmounted", async () => {
    const view = renderDialog();
    await act(async () => {});

    view.unmount();

    expect(mocks.unlisten).toHaveBeenCalledTimes(1);
  });

  it("enters batch mode after importing a text URL list", async () => {
    const { readFileAsText } = await import("@/lib/local-file");
    vi.mocked(readFileAsText).mockResolvedValue("https://example.com/a.zip\nhttps://example.com/b.zip\n");
    mocks.openFilePicker.mockResolvedValue({ path: "C:/urls.txt", name: "urls.txt" });
    mocks.importUrls.mockResolvedValue({
      items: [
        {
          inputUrl: "https://example.com/a.zip",
          normalizedUrl: "https://example.com/a.zip",
          duplicate: false,
          valid: true,
          fileName: "a.zip",
          totalSize: "1",
          contentType: null,
          supportsResume: true,
          errorMessage: null,
          task: null,
        },
        {
          inputUrl: "https://example.com/b.zip",
          normalizedUrl: "https://example.com/b.zip",
          duplicate: false,
          valid: true,
          fileName: "b.zip",
          totalSize: "1",
          contentType: null,
          supportsResume: true,
          errorMessage: null,
          task: null,
        },
      ],
      createdCount: 0,
      failedCount: 0,
      duplicateCount: 0,
    });

    renderDialog();
    await act(async () => {});

    fireEvent.click(screen.getByTitle("newDownload.chooseFile"));

    await act(async () => {});
    await act(async () => {});

    expect(mocks.importUrls).toHaveBeenCalledWith(
      expect.objectContaining({
        input: expect.stringContaining("https://example.com/a.zip"),
        probe: true,
        create: false,
      }),
    );
    expect(screen.getByDisplayValue(/https:\/\/example\.com\/a\.zip/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "newDownload.createBatch" })).toBeInTheDocument();
  });

  it("keeps batch-only overrides collapsed until more options is opened", () => {
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));

    const toggle = screen.getByRole("button", { name: "newDownload.advancedOptions" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("newDownload.useCredentials")).not.toBeInTheDocument();

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("newDownload.useCredentials")).toBeInTheDocument();
    expect(screen.getByLabelText("newDownload.priority")).toBeInTheDocument();
  });

  it("marks a draft dirty when it only contains an advanced batch override", () => {
    const onDraftStateChange = vi.fn();
    render(
      <TooltipProvider>
        <NewDownloadDialog open onOpenChange={vi.fn()} onCreated={vi.fn()} onDraftStateChange={onDraftStateChange} />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    fireEvent.click(screen.getByRole("button", { name: "newDownload.advancedOptions" }));
    fireEvent.click(screen.getByLabelText("newDownload.useCredentials"));

    expect(onDraftStateChange).toHaveBeenLastCalledWith(true);
  });

  it("disables Cancel while a batch creation is in flight", () => {
    const pending = deferred<BatchImportResult>();
    mocks.importUrls.mockReturnValue(pending.promise);
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), {
      target: { value: "https://example.com/creating.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "newDownload.createBatch" }));

    expect(screen.getByRole("button", { name: "newDownload.cancel" })).toBeDisabled();
  });

  it("keeps every batch result and retries only the remaining failures", async () => {
    const task = { id: "created-1", url: "https://example.com/1.zip", fileName: "1.zip" } as Task;
    const result = {
      items: [
        {
          inputUrl: "https://example.com/1.zip",
          normalizedUrl: "https://example.com/1.zip",
          duplicate: false,
          valid: true,
          fileName: "1.zip",
          totalSize: "1",
          contentType: null,
          supportsResume: true,
          errorMessage: null,
          task,
        },
        {
          inputUrl: "https://example.com/2.zip",
          normalizedUrl: "https://example.com/2.zip",
          duplicate: false,
          valid: false,
          fileName: null,
          totalSize: null,
          contentType: null,
          supportsResume: false,
          errorMessage: "failed-2",
          task: null,
        },
        {
          inputUrl: "https://example.com/3.zip",
          normalizedUrl: "https://example.com/3.zip",
          duplicate: true,
          valid: true,
          fileName: null,
          totalSize: null,
          contentType: null,
          supportsResume: false,
          errorMessage: "duplicate-3",
          task: null,
        },
        {
          inputUrl: "https://example.com/4.zip",
          normalizedUrl: "https://example.com/4.zip",
          duplicate: false,
          valid: false,
          fileName: null,
          totalSize: null,
          contentType: null,
          supportsResume: false,
          errorMessage: "failed-4",
          task: null,
        },
        {
          inputUrl: "https://example.com/5.zip",
          normalizedUrl: "https://example.com/5.zip",
          duplicate: false,
          valid: true,
          fileName: "5.zip",
          totalSize: "1",
          contentType: null,
          supportsResume: true,
          errorMessage: null,
          task,
        },
        {
          inputUrl: "https://example.com/6.zip",
          normalizedUrl: "https://example.com/6.zip",
          duplicate: false,
          valid: false,
          fileName: null,
          totalSize: null,
          contentType: null,
          supportsResume: false,
          errorMessage: "failed-6",
          task: null,
        },
      ],
      createdCount: 2,
      failedCount: 3,
      duplicateCount: 1,
    };
    const retryResult = { ...result, items: result.items.slice(1), createdCount: 0 };
    mocks.importUrls.mockResolvedValueOnce(result).mockResolvedValueOnce(retryResult);
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    const input =
      "https://example.com/1.zip\nhttps://example.com/2.zip\nhttps://example.com/3.zip\nhttps://example.com/4.zip\nhttps://example.com/5.zip\nhttps://example.com/6.zip";
    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), { target: { value: input } });
    fireEvent.click(screen.getByRole("button", { name: "newDownload.createBatch" }));
    await act(async () => {});

    expect(screen.getByText("https://example.com/6.zip")).toBeInTheDocument();
    expect(screen.getByText("failed-2")).toBeInTheDocument();
    expect(screen.getByLabelText("newDownload.batchUrls")).not.toHaveValue(
      expect.stringContaining("https://example.com/1.zip"),
    );

    fireEvent.click(screen.getByRole("button", { name: "newDownload.batchRetryFailed" }));
    await act(async () => {});
    expect(mocks.importUrls).toHaveBeenLastCalledWith(
      expect.objectContaining({
        create: true,
        input: expect.stringContaining("https://example.com/6.zip"),
      }),
    );
    const lastInput = mocks.importUrls.mock.calls[mocks.importUrls.mock.calls.length - 1]?.[0].input as string;
    expect(lastInput).not.toContain("https://example.com/1.zip");
  });

  it("blocks batch create during preview and drops a preview made stale by editing", async () => {
    const pending = deferred<BatchImportResult>();
    mocks.importUrls.mockReturnValue(pending.promise);
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "newDownload.modeBatch" }));
    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), {
      target: { value: "https://example.com/old.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "newDownload.previewBatch" }));
    expect(screen.getByRole("button", { name: "newDownload.createBatch" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("newDownload.batchUrls"), {
      target: { value: "https://example.com/new.zip" },
    });
    await act(async () =>
      pending.resolve({
        items: [],
        createdCount: 0,
        failedCount: 0,
        duplicateCount: 0,
      }),
    );

    expect(screen.queryByText("newDownload.batchSummary")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "newDownload.createBatch" })).not.toBeDisabled();
  });
});

describe("NewDownloadDialog HLS track picker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.phaseHandler = undefined;
    useSettingsStore.setState({ settings: null, loading: false, error: null });
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("caps the rendered track rows and reveals the rest on demand", async () => {
    // Long-tail manifests carry hundreds of renditions; the picker must not
    // render an unbounded checkbox wall. Collapsed shows the first 6 rows.
    const tracks = Array.from({ length: 8 }, (_, i) => ({
      kind: "AUDIO",
      groupId: "audio",
      name: `Track ${i + 1}`,
      language: null,
      default: false,
      autoSelect: false,
      uri: `https://example.com/audio-${i + 1}.m3u8`,
    }));
    const probe: ProbeTaskPayload = {
      ...makeProbe("https://example.com/live.m3u8", "live.m3u8"),
      protocol: "hls",
      hlsAudioTracks: tracks,
      hlsSubtitleTracks: [],
    };
    mocks.probeTask.mockResolvedValue(probe);
    renderDialog();

    await startAutomaticProbe("https://example.com/live.m3u8");

    expect(screen.getAllByRole("checkbox")).toHaveLength(6);
    const toggle = screen.getByRole("button", { name: "newDownload.hlsShowMoreTracks" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(toggle);

    expect(screen.getAllByRole("checkbox")).toHaveLength(8);
    expect(screen.getByRole("button", { name: "newDownload.hlsFewerTracks" })).toBeInTheDocument();
  });

  it("keeps a selected track beyond the cap visible while collapsed", async () => {
    // An auto-selected default must never hide its own checked state, so the
    // collapsed picker keeps selected rows in view next to the first 6.
    const tracks = Array.from({ length: 8 }, (_, i) => ({
      kind: "AUDIO",
      groupId: "audio",
      name: `Track ${i + 1}`,
      language: null,
      default: i === 7,
      autoSelect: i === 7,
      uri: `https://example.com/audio-${i + 1}.m3u8`,
    }));
    const probe: ProbeTaskPayload = {
      ...makeProbe("https://example.com/live.m3u8", "live.m3u8"),
      protocol: "hls",
      hlsAudioTracks: tracks,
      hlsSubtitleTracks: [],
    };
    mocks.probeTask.mockResolvedValue(probe);
    renderDialog();

    await startAutomaticProbe("https://example.com/live.m3u8");

    const boxes = screen.getAllByRole("checkbox");
    expect(boxes).toHaveLength(7);
    expect(boxes.some((box) => box.getAttribute("aria-label") === "Track 8")).toBe(true);
    expect(screen.getByRole("button", { name: "newDownload.hlsShowMoreTracks" })).toBeInTheDocument();
  });
});

// UX-30: closing a dirty draft must not silently destroy work — the dialog
// stays mounted in the shell, so the same state is still there on reopen;
// closing while a create is in flight hides the dialog with an explicit
// "still running" notice instead of suggesting cancellation.
describe("NewDownloadDialog close semantics", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.phaseHandler = undefined;
    mocks.onProbePhase.mockImplementation(async (handler: (payload: ProbePhasePayload) => void) => {
      mocks.phaseHandler = handler;
      return mocks.unlisten;
    });
    useSettingsStore.setState({ settings: null, loading: false, error: null });
    useToastStore.setState({ toasts: [] });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("keeps the draft after close and announces it once", async () => {
    // Spying on the store method: the toast is deferred while the modal owns
    // focus, so asserting the call (not the rendered stack) is the stable
    // contract — the deferral mechanism itself has its own coverage.
    const addToast = vi.spyOn(useToastStore.getState(), "addToast");
    const view = renderDialog();
    fireEvent.change(screen.getByLabelText("newDownload.url"), {
      target: { value: "https://example.com/draft.zip" },
    });

    // The Cancel button goes through the same close guard as Escape/overlay.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "newDownload.cancel" }));
    });
    expect(view.onOpenChange).toHaveBeenCalledWith(false);
    expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ title: "newDownload.draftKeptTitle" }));

    // Close then reopen on the same mounted instance: the draft is still there.
    view.rerender(
      <TooltipProvider>
        <NewDownloadDialog open={false} onOpenChange={view.onOpenChange} onCreated={view.onCreated} />
      </TooltipProvider>,
    );
    view.rerender(
      <TooltipProvider>
        <NewDownloadDialog open onOpenChange={view.onOpenChange} onCreated={view.onCreated} />
      </TooltipProvider>,
    );
    expect(screen.getByLabelText("newDownload.url")).toHaveValue("https://example.com/draft.zip");
  });

  it("announces a still-running create instead of pretending it was canceled", async () => {
    const addToast = vi.spyOn(useToastStore.getState(), "addToast");
    const pending = deferred<Task>();
    mocks.createTask.mockReturnValueOnce(pending.promise);
    mocks.probeTask.mockResolvedValue(makeProbe("https://example.com/busy.zip", "busy.zip"));
    const view = renderDialog();
    await startAutomaticProbe("https://example.com/busy.zip");

    // Submit the form so `submitting` is true while the IPC is in flight.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "newDownload.start" }));
    });

    // Escape while submitting hides the dialog and surfaces the explicit
    // still-running notice (not an implied cancel).
    await act(async () => {
      fireEvent.keyDown(document, { key: "Escape" });
    });
    expect(view.onOpenChange).toHaveBeenCalledWith(false);
    expect(addToast).toHaveBeenCalledWith(expect.objectContaining({ title: "newDownload.closeSubmittingTitle" }));
    // The in-flight IPC was never interrupted by the close.
    expect(mocks.createTask).toHaveBeenCalledTimes(1);

    await act(async () => pending.resolve({ id: "done" } as Task));
  });
});

// UX-25: a rejected directory picker must surface the failure in the
// dialog's error region instead of dying as a silent unhandled rejection.
describe("NewDownloadDialog picker failure feedback", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.openDirectoryPicker.mockReset();
    mocks.openFilePicker.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the error region when the directory picker rejects", async () => {
    mocks.openDirectoryPicker.mockRejectedValueOnce(new Error("picker denied"));
    renderDialog();

    await act(async () => {
      fireEvent.click(screen.getAllByRole("button", { name: "newDownload.chooseDirectory" })[0]);
    });

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(mocks.openDirectoryPicker).toHaveBeenCalledTimes(1);
  });
});
