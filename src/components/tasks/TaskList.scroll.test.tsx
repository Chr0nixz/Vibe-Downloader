import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";

import { TaskList } from "./TaskList";

// Hoisted so the assertion survives the per-render mock object recreation.
const scrollToIndex = vi.hoisted(() => vi.fn());
const listTasksCursor = vi.hoisted(() => vi.fn());

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
    scrollToIndex,
    measure: vi.fn(),
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

function page(items: Task[], nextCursor: string | null) {
  return { items, minimumTotal: items.length, nextCursor, filterOptions: { sources: [], failureCategories: [] } };
}

describe("TaskList selection scroll (UX-17)", () => {
  beforeEach(() => {
    resetListQueryEpochForTests();
    scrollToIndex.mockClear();
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
    vi.clearAllMocks();
  });

  it("does not re-scroll to the selection after an append load", async () => {
    listTasksCursor.mockResolvedValue(page([sampleTask("a", "a.bin"), sampleTask("b", "b.bin")], "cursor-1"));
    renderList();

    await waitFor(() => expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b"]));

    // A selection change scrolls exactly once (whether it came from the list's
    // own first-load auto-select or this explicit call).
    await act(async () => {
      useTaskUIStore.getState().selectTask("a");
    });
    await waitFor(() => expect(scrollToIndex).toHaveBeenCalledTimes(1));

    // Append a page: taskIds (and thus `filtered`) get a fresh identity, but
    // the selection is unchanged, so the viewport must stay where it is.
    await act(async () => {
      useTaskDataStore.getState().setTaskCursorPage(
        [sampleTask("c", "c.bin"), sampleTask("d", "d.bin")],
        4,
        null,
        {
          sources: [],
          failureCategories: [],
        },
        true,
      );
    });
    expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b", "c", "d"]);
    expect(scrollToIndex).toHaveBeenCalledTimes(1);
  });

  it("scrolls to the row when the selection changes", async () => {
    listTasksCursor.mockResolvedValue(
      page([sampleTask("a", "a.bin"), sampleTask("b", "b.bin"), sampleTask("c", "c.bin")], null),
    );
    renderList();

    await waitFor(() => expect(useTaskDataStore.getState().taskIds).toEqual(["a", "b", "c"]));

    const before = scrollToIndex.mock.calls.length;
    await act(async () => {
      useTaskUIStore.getState().selectTask("c");
    });
    await waitFor(() => expect(scrollToIndex).toHaveBeenCalledTimes(before + 1));
    expect(scrollToIndex).toHaveBeenLastCalledWith(2, { align: "center" });
  });
});
