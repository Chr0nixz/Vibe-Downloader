import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";

import { TaskList } from "./TaskList";

const listTasksCursor = vi.fn();

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

vi.mock("motion/react", () => ({
  useReducedMotion: () => true,
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [],
    getTotalSize: () => 0,
    scrollToOffset: vi.fn(),
    scrollToIndex: vi.fn(),
    measureElement: vi.fn(),
  }),
}));

vi.mock("@/lib/tauri", () => ({
  listTasksCursor: (...args: unknown[]) => listTasksCursor(...args),
}));

vi.mock("@/components/tasks/TaskRow", () => ({
  TaskRow: () => null,
}));

vi.mock("@/components/tasks/TaskContextMenu", () => ({
  ListContextMenu: ({ children }: { children: React.ReactNode }) => children,
}));

import { resetListQueryEpochForTests } from "@/lib/list-query-epoch";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";

function sampleTask(id: string, fileName: string): Task {
  return {
    id,
    url: `https://example.com/${id}`,
    finalUrl: `https://example.com/${id}`,
    protocol: "http",
    taskKind: "single_file",
    fileName,
    saveDir: "/tmp",
    tempPath: null,
    finalPath: `/tmp/${fileName}`,
    totalSize: 10,
    downloadedBytes: 0,
    status: "queued",
    etag: null,
    lastModified: null,
    contentType: null,
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
    createdAt: "2024-01-01T00:00:00Z",
    updatedAt: "2024-01-01T00:00:00Z",
    files: [],
  };
}

const noop = async () => {};

function renderList() {
  return render(
    <TaskList
      onToggleTransfer={noop}
      onRetry={noop}
      onFinishLiveRecording={noop}
      onOpenFile={noop}
      onOpenFolder={noop}
      onResolveAttention={noop}
      onDelete={noop}
      onNewDownload={noop}
      onBulkPause={noop}
      onBulkResume={noop}
      onBulkRetry={noop}
      onBulkDelete={noop}
      onBulkOpenFolder={noop}
      onBulkExport={noop}
      onOpenOnboarding={noop}
      onUpdateQueueOptions={async () => true}
    />,
  );
}

describe("TaskList query race (ARC-07)", () => {
  beforeEach(() => {
    resetListQueryEpochForTests();
    listTasksCursor.mockReset();
    useTaskDataStore.setState({
      tasks: [],
      taskIds: [],
      taskById: {},
      taskIndexById: {},
      nextCursor: null,
      hasMore: false,
      loading: false,
      error: null,
      total: 0,
      filterOptions: { sources: [], failureCategories: [] },
    });
    useTaskUIStore.setState({
      nav: "all",
      search: "",
      selectedId: null,
      selectedIds: [],
      sortKey: "updated_at",
      sortDirection: "desc",
      filters: { fileType: "all", source: "all", failure: "all", resume: "all" },
      pendingDeleteIds: [],
    });
  });

  afterEach(() => {
    resetListQueryEpochForTests();
  });

  it("ignores stale listTasksCursor response when nav changes mid-flight", async () => {
    let resolveAll!: (value: unknown) => void;
    const allPromise = new Promise((resolve) => {
      resolveAll = resolve;
    });
    listTasksCursor
      .mockImplementationOnce(() => allPromise)
      .mockResolvedValue({
        items: [sampleTask("completed-1", "done.bin")],
        minimumTotal: 1,
        nextCursor: null,
        filterOptions: { sources: [], failureCategories: [] },
      });

    renderList();
    await waitFor(() => expect(listTasksCursor).toHaveBeenCalledTimes(1));

    await act(async () => {
      useTaskUIStore.getState().setNav("completed");
    });

    // Replace is still in flight → only pendingReload is set; second request waits.
    expect(listTasksCursor).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveAll({
        items: [sampleTask("all-stale", "stale.bin")],
        minimumTotal: 1,
        nextCursor: null,
        filterOptions: { sources: [], failureCategories: [] },
      });
    });

    await waitFor(() => expect(listTasksCursor).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      expect(useTaskDataStore.getState().taskIds).toEqual(["completed-1"]);
    });
    expect(useTaskDataStore.getState().taskIds).not.toContain("all-stale");
  });
});
