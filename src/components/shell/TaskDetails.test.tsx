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
  getTaskProxySettings: vi.fn(),
  getTorrentRuntimeSnapshot: vi.fn(),
  listMetalinkMirrors: vi.fn(),
  listSegmentsPage: vi.fn(),
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
  getTaskProxySettings: mocks.getTaskProxySettings,
  getTorrentRuntimeSnapshot: mocks.getTorrentRuntimeSnapshot,
  listMetalinkMirrors: mocks.listMetalinkMirrors,
  listSegmentsPage: mocks.listSegmentsPage,
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
    mocks.listTaskEventsPage.mockResolvedValue({ items: [], nextCursor: null });
    mocks.listTaskRequestsPage.mockResolvedValue({ items: [], nextCursor: null });
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
