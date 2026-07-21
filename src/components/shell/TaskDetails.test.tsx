import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useTaskDataStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { TaskDetails } from "./TaskDetails";

const layout = vi.hoisted(() => ({ compact: false }));
const mocks = vi.hoisted(() => ({
  computeFileHash: vi.fn(),
  finishLiveRecording: vi.fn(),
  getSegmentSummary: vi.fn(),
  getTaskProxySettings: vi.fn(),
  getTorrentRuntimeSnapshot: vi.fn(),
  listDashSegmentsPage: vi.fn(),
  listHlsSegmentsPage: vi.fn(),
  listMetalinkMirrors: vi.fn(),
  listSegmentsPage: vi.fn(),
  listSftpKnownHosts: vi.fn(),
  listTaskEventsPage: vi.fn(),
  listTaskRequestsPage: vi.fn(),
  onTaskUpdated: vi.fn(),
  retryTaskWithMirror: vi.fn(),
  updateTaskProxySettings: vi.fn(),
  updateTaskTransferOptions: vi.fn(),
  updateTorrentFileSelection: vi.fn(),
  updateTorrentSeeding: vi.fn(),
  verifyTaskHash: vi.fn(),
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

vi.mock("@/hooks/use-shell-layout", () => ({
  useIsCompactShell: () => layout.compact,
}));

vi.mock("@/lib/tauri", () => ({
  computeFileHash: mocks.computeFileHash,
  finishLiveRecording: mocks.finishLiveRecording,
  getSegmentSummary: mocks.getSegmentSummary,
  getTaskProxySettings: mocks.getTaskProxySettings,
  getTorrentRuntimeSnapshot: mocks.getTorrentRuntimeSnapshot,
  listDashSegmentsPage: mocks.listDashSegmentsPage,
  listHlsSegmentsPage: mocks.listHlsSegmentsPage,
  listMetalinkMirrors: mocks.listMetalinkMirrors,
  listSegmentsPage: mocks.listSegmentsPage,
  listSftpKnownHosts: mocks.listSftpKnownHosts,
  listTaskEventsPage: mocks.listTaskEventsPage,
  listTaskRequestsPage: mocks.listTaskRequestsPage,
  onTaskUpdated: mocks.onTaskUpdated,
  retryTaskWithMirror: mocks.retryTaskWithMirror,
  updateTaskProxySettings: mocks.updateTaskProxySettings,
  updateTaskTransferOptions: mocks.updateTaskTransferOptions,
  updateTorrentFileSelection: mocks.updateTorrentFileSelection,
  updateTorrentSeeding: mocks.updateTorrentSeeding,
  verifyTaskHash: mocks.verifyTaskHash,
}));

function makeTask(id: string, fileName = `${id}.zip`, overrides: Partial<Task> = {}): Task {
  const now = "2026-07-14T00:00:00.000Z";
  return {
    id,
    url: `https://example.com/${fileName}`,
    finalUrl: null,
    protocol: "https",
    taskKind: "single_file",
    fileName,
    saveDir: "D:\\Downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 1024,
    downloadedBytes: 256,
    status: "paused",
    etag: null,
    lastModified: null,
    contentType: "application/zip",
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 0,
    speedBps: 0,
    taskSpeedLimitBps: null,
    priority: "normal",
    queuePosition: "0",
    categoryKey: null,
    obeySchedule: true,
    healthSummary: null,
    errorMessage: null,
    errorCode: null,
    recoveryActions: [],
    retryAfterAt: null,
    failureCategory: null,
    expectedHashSha256: null,
    actualHashSha256: null,
    hashStatus: "not_requested",
    hashError: null,
    hashVerifiedAt: null,
    checksums: [],
    files: [],
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function seedTasks(tasks: Task[]) {
  useTaskDataStore.setState({
    tasks,
    taskIds: tasks.map((task) => task.id),
    taskById: Object.fromEntries(tasks.map((task) => [task.id, task])),
    taskIndexById: Object.fromEntries(tasks.map((task, index) => [task.id, index])),
  });
}

function renderDetails(taskId: string, onClose = vi.fn()) {
  const view = render(
    <TooltipProvider>
      <TaskDetails taskId={taskId} open onClose={onClose} onResolveAttention={vi.fn()} />
    </TooltipProvider>,
  );
  return { ...view, onClose };
}

describe("TaskDetails", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    layout.compact = false;
    mocks.listSegmentsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.listHlsSegmentsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.listDashSegmentsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.listTaskEventsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.listTaskRequestsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.getSegmentSummary.mockResolvedValue({
      total: 0,
      active: 0,
      completed: 0,
      failed: 0,
      downloadedBytes: "0",
      speedBps: "0",
    });
    mocks.listSftpKnownHosts.mockResolvedValue([]);
    mocks.finishLiveRecording.mockResolvedValue({});
    mocks.onTaskUpdated.mockResolvedValue(mocks.unlisten);
    // ScrollArea (Radix) needs ResizeObserver in jsdom.
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
  });

  it("loads only the data required by the selected detail tab", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-tabs", "tabs.zip");
    seedTasks([task]);
    renderDetails(task.id);

    expect(screen.getByRole("complementary")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "tabs.zip" })).toBeInTheDocument();
    expect(mocks.listSegmentsPage).not.toHaveBeenCalled();

    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await waitFor(() => expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(1));
    expect(screen.getByText("taskDetails.noChunks")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "taskDetails.logs" }));
    await waitFor(() => expect(mocks.listTaskEventsPage).toHaveBeenCalledTimes(1));
    expect(screen.getByText("taskDetails.noLogs")).toBeInTheDocument();
  });

  it("loads torrent runtime snapshot only on the overview tab", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-bt", "bt.torrent", { protocol: "bt", status: "downloading" });
    mocks.getTorrentRuntimeSnapshot.mockResolvedValue({
      taskId: task.id,
      metadataStatus: "ready",
      completedPieces: "0",
      verifiedPieces: "0",
      pieceCount: "1",
      pieceBitfieldBase64: null,
      peerCount: "0",
      seedCount: null,
      dhtStatus: null,
      trackers: [],
      uploadBytes: "0",
      uploadSpeedBps: "0",
      ratio: 0,
      seedingEnabled: false,
      seedingState: "disabled",
      seedRatioLimit: null,
      seedTimeLimitSeconds: null,
      lastErrorCode: null,
      lastErrorMessage: null,
      updatedAt: task.updatedAt,
    });
    seedTasks([task]);
    renderDetails(task.id);

    await waitFor(() => expect(mocks.getTorrentRuntimeSnapshot).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    expect(screen.queryByRole("tab", { name: "taskDetails.segments" })).not.toBeInTheDocument();
    await waitFor(() => expect(mocks.listTaskRequestsPage).toHaveBeenCalled());
    expect(mocks.listSegmentsPage).not.toHaveBeenCalled();
    const callsAfterLeave = mocks.getTorrentRuntimeSnapshot.mock.calls.length;
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(mocks.getTorrentRuntimeSnapshot).toHaveBeenCalledTimes(callsAfterLeave);
  });

  it("loads HLS segments instead of placeholder task_segments", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-hls", "live.m3u8", {
      protocol: "hls",
      status: "downloading",
      url: "https://cdn.example.com/live.m3u8",
    });
    mocks.listHlsSegmentsPage.mockResolvedValue({
      items: [
        {
          id: "hs-1",
          mediaSequence: "10",
          discontinuitySequence: "0",
          uri: "https://cdn.example.com/seg10.ts",
          durationMs: "4000",
          status: "completed",
          retryCount: 0,
          lastError: null,
          downloadedBytes: "1000",
        },
      ],
      nextCursor: null,
    });
    seedTasks([task]);
    renderDetails(task.id);

    expect(screen.getByRole("button", { name: "actions.finishRecording" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await waitFor(() => expect(mocks.listHlsSegmentsPage).toHaveBeenCalled());
    expect(mocks.listSegmentsPage).not.toHaveBeenCalled();
    expect(screen.getByText("#10")).toBeInTheDocument();
  });

  it("loads DASH segments instead of placeholder task_segments", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-dash", "movie.mpd", {
      protocol: "dash",
      status: "downloading",
      url: "https://cdn.example.com/movie.mpd",
    });
    mocks.listDashSegmentsPage.mockResolvedValue({
      items: [
        {
          id: "ds-1",
          trackKind: "video",
          segmentIndex: "3",
          uri: "https://cdn.example.com/v3.m4s",
          status: "failed",
          retryCount: 1,
          lastError: "HTTP 404",
          downloadedBytes: "0",
        },
      ],
      nextCursor: null,
    });
    seedTasks([task]);
    renderDetails(task.id);

    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await waitFor(() => expect(mocks.listDashSegmentsPage).toHaveBeenCalled());
    expect(mocks.listSegmentsPage).not.toHaveBeenCalled();
    expect(mocks.listHlsSegmentsPage).not.toHaveBeenCalled();
    expect(screen.getByText("video #3")).toBeInTheDocument();
    expect(screen.getByText("HTTP 404")).toBeInTheDocument();
  });

  it("shows Metalink file progress on Overview", async () => {
    const task = makeTask("task-metalink", "pack.meta4", {
      protocol: "metalink",
      status: "downloading",
      url: "https://cdn.example.com/pack.meta4",
      files: [
        {
          id: "file-1",
          taskId: "task-metalink",
          relativePath: "docs/readme.txt",
          fileName: "readme.txt",
          saveDir: "D:\\Downloads",
          tempPath: null,
          finalPath: null,
          totalSize: 200,
          downloadedBytes: 50,
          selected: true,
          status: "downloading",
          contentType: null,
        },
      ],
    });
    mocks.listMetalinkMirrors.mockResolvedValue([]);
    seedTasks([task]);
    renderDetails(task.id);

    await waitFor(() => expect(screen.getByText("docs/readme.txt")).toBeInTheDocument());
    expect(screen.getByText("taskDetails.metalinkFilesHeader")).toBeInTheDocument();
  });

  it("shows FTP/SFTP overview panel and loads segment summary", async () => {
    const task = makeTask("task-ftp", "file.bin", {
      protocol: "ftp",
      status: "paused",
      url: "ftp://example.com/file.bin",
      connectionCount: 2,
      supportsResume: true,
      supportsParallel: true,
    });
    mocks.getSegmentSummary.mockResolvedValue({
      total: 4,
      active: 1,
      completed: 2,
      failed: 1,
      downloadedBytes: "100",
      speedBps: "0",
    });
    seedTasks([task]);
    renderDetails(task.id);

    await waitFor(() => expect(mocks.getSegmentSummary).toHaveBeenCalledWith(task.id));
    expect(screen.getByText("taskDetails.ftpSftpRuntime")).toBeInTheDocument();
    expect(screen.getByText("taskDetails.ftpSegmentSummary")).toBeInTheDocument();
  });

  it("hides If-Range for non-HTTP request methods", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-ftp-req", "file.bin", { protocol: "ftp", status: "paused" });
    mocks.listTaskRequestsPage.mockResolvedValue({
      items: [
        {
          id: "req-1",
          taskId: task.id,
          method: "FTP RETR",
          url: "ftp://example.com/file.bin",
          statusCode: 226,
          rangeHeader: "REST 0-99",
          ifRangeHeader: "should-hide",
          contentLength: "100",
          durationMs: "12",
          retryCount: 0,
          etag: "etag-should-hide",
          errorMessage: null,
          createdAt: task.createdAt,
          lastModified: null,
        },
      ],
      nextCursor: null,
    });
    seedTasks([task]);
    renderDetails(task.id);
    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await user.click(screen.getByRole("tab", { name: "taskDetails.requests" }));
    await waitFor(() => expect(screen.getByText("FTP RETR")).toBeInTheDocument());
    expect(screen.queryByText("taskDetails.requestIfRange")).not.toBeInTheDocument();
    expect(screen.queryByText(/ETag/)).not.toBeInTheDocument();
  });

  it("stops segment polling when switching to the Requests sub-tab", async () => {
    const user = userEvent.setup();
    const task = makeTask("task-req-tab", "requests.zip", { status: "downloading" });
    seedTasks([task]);
    renderDetails(task.id);

    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await waitFor(() => expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(1));

    await user.click(screen.getByRole("tab", { name: "taskDetails.requests" }));
    await waitFor(() => expect(mocks.listTaskRequestsPage).toHaveBeenCalledTimes(1));

    const callsAfterSwitch = mocks.listSegmentsPage.mock.calls.length;
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(callsAfterSwitch);
  });

  it("skips overlapping segments polls while a request is in flight", async () => {
    const user = userEvent.setup();
    let release!: (value: { items: never[]; nextCursor: null }) => void;
    const gate = new Promise<{ items: never[]; nextCursor: null }>((resolve) => {
      release = resolve;
    });
    mocks.listSegmentsPage.mockImplementation(() => gate);

    const task = makeTask("task-inflight", "inflight.zip", { status: "downloading" });
    seedTasks([task]);
    renderDetails(task.id);

    await user.click(screen.getByRole("tab", { name: "taskDetails.diagnostics" }));
    await waitFor(() => expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(1));

    // Interval ticks cannot start a second request while the first is pending.
    await new Promise((resolve) => setTimeout(resolve, 2_100));
    expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(1);

    release({ items: [], nextCursor: null });
    await waitFor(() => expect(mocks.listSegmentsPage).toHaveBeenCalledTimes(1));
  });

  it("resets the selected tab when switching to another task", async () => {
    const user = userEvent.setup();
    const first = makeTask("task-first", "first.zip");
    const second = makeTask("task-second", "second.zip");
    seedTasks([first, second]);
    const view = renderDetails(first.id);

    await user.click(screen.getByRole("tab", { name: "taskDetails.logs" }));
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "taskDetails.logs" })).toHaveAttribute("data-state", "active"),
    );

    view.rerender(
      <TooltipProvider>
        <TaskDetails taskId={second.id} open onClose={view.onClose} onResolveAttention={vi.fn()} />
      </TooltipProvider>,
    );

    await waitFor(() => expect(screen.getByRole("heading", { name: "second.zip" })).toBeInTheDocument());
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "taskDetails.overview" })).toHaveAttribute("data-state", "active"),
    );
  });

  it("uses a focus-managed drawer and closes it on compact layouts", async () => {
    const user = userEvent.setup();
    layout.compact = true;
    const task = makeTask("task-drawer", "drawer.zip");
    seedTasks([task]);
    const { onClose } = renderDetails(task.id);

    const drawer = screen.getByRole("dialog", { name: "drawer.zip" });
    expect(drawer).toBeInTheDocument();
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    const close = screen.getByRole("button", { name: "taskDetails.close" });
    await waitFor(() => expect(close).toHaveFocus());
    await user.click(close);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows a close button in the wide detail sidebar", async () => {
    const user = userEvent.setup();
    layout.compact = false;
    const task = makeTask("task-wide-close", "wide.zip");
    seedTasks([task]);
    const { onClose } = renderDetails(task.id);

    expect(screen.getByRole("complementary")).toBeInTheDocument();
    const close = screen.getByRole("button", { name: "taskDetails.close" });
    await user.click(close);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("toggles torrent seeding without clearing ratio/time limits", async () => {
    const user = userEvent.setup();
    const task = {
      ...makeTask("task-bt-seed", "seed.torrent"),
      protocol: "bt" as const,
      url: "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    };
    seedTasks([task]);
    mocks.getTorrentRuntimeSnapshot.mockResolvedValue({
      taskId: task.id,
      metadataStatus: "ready",
      completedPieces: "1",
      verifiedPieces: "1",
      pieceCount: "2",
      pieceBitfieldBase64: null,
      peerCount: "3",
      seedCount: null,
      dhtStatus: null,
      trackers: [
        {
          url: "udp://tracker.example:6969/announce",
          status: "configured",
          source: "configured",
          updatedAt: task.updatedAt,
          lastError: null,
        },
      ],
      uploadBytes: "0",
      uploadSpeedBps: "0",
      ratio: 0,
      seedingEnabled: false,
      seedingState: "disabled",
      seedRatioLimit: 1.5,
      seedTimeLimitSeconds: "3600",
      lastErrorCode: null,
      lastErrorMessage: null,
      updatedAt: task.updatedAt,
    });
    mocks.updateTorrentSeeding.mockResolvedValue(task);

    renderDetails(task.id);

    await waitFor(() => expect(screen.getByText("taskDetails.btTrackersConfiguredOnly")).toBeInTheDocument());
    expect(screen.getByText("taskDetails.btPeersOnly")).toBeInTheDocument();

    const seedingSwitch = screen.getByRole("switch", { name: "taskDetails.btSeeding" });
    await user.click(seedingSwitch);

    await waitFor(() => expect(mocks.updateTorrentSeeding).toHaveBeenCalled());
    expect(mocks.updateTorrentSeeding).toHaveBeenCalledWith(
      expect.objectContaining({
        taskId: task.id,
        enabled: true,
        updateLimits: false,
      }),
    );
  });
});
