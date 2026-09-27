import { beforeEach, describe, expect, it } from "vitest";

import type { Task } from "@/types/task";

import { useTaskDataStore } from "./task-data-store";
import { useTaskUIStore } from "./task-ui-store";

function makeTask(overrides: Partial<Task> = {}): Task {
  const now = "2026-01-01T00:00:00.000Z";
  return {
    id: "task-1",
    url: "https://example.com/file.bin",
    finalUrl: null,
    protocol: "http",
    taskKind: "single_file",
    fileName: "file.bin",
    saveDir: "D:\\Downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 100,
    downloadedBytes: 0,
    status: "completed",
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
    files: [],
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function pageTasks(prefix: string, count: number, status: Task["status"] = "completed"): Task[] {
  return Array.from({ length: count }, (_, i) =>
    makeTask({ id: `${prefix}-${i}`, status, fileName: `${prefix}-${i}.bin` }),
  );
}

const EMPTY_FILTER_OPTIONS = { sources: [], failureCategories: [] };

describe("task-data-store entity cache (PERF-18 / R26-P02)", () => {
  beforeEach(() => {
    useTaskUIStore.setState({
      nav: "all",
      search: "",
      filters: { fileType: "all", source: "all", failure: "all", resume: "all" },
      sortKey: "updated_at",
      sortDirection: "desc",
      selectedId: null,
      selectedIds: [],
      selectionAnchorId: null,
      pendingDeleteIds: [],
      detailOpen: false,
    });
    useTaskDataStore.setState({
      tasks: [],
      taskIds: [],
      taskById: {},
      taskIndexById: {},
      total: 0,
      viewReloadToken: 0,
      nextCursor: null,
      hasMore: false,
      loading: false,
      error: null,
      filterOptions: { sources: [], failureCategories: [] },
      expandedTaskIds: [],
    });
  });

  it("bounds the entity cache across repeated query replacements", () => {
    // Probe reproduction from the review: ten distinct 100-task pages replaced
    // in sequence used to leave 1000 entities cached. Now the cache must stay
    // bounded (current page pinned + cap on the rest).
    const store = useTaskDataStore.getState();
    for (let page = 0; page < 10; page += 1) {
      store.setTaskCursorPage(pageTasks(`q${page}`, 100), 100, null, EMPTY_FILTER_OPTIONS, false);
    }
    const state = useTaskDataStore.getState();
    expect(state.taskIds).toHaveLength(100);
    const cached = Object.keys(state.taskById).length;
    expect(cached).toBeLessThan(1000);
    // The visible page must always survive eviction.
    for (const id of state.taskIds) {
      expect(state.taskById[id], `visible ${id} must stay cached`).toBeDefined();
    }
  });

  it("pins selected, expanded, and active entities during eviction", () => {
    const store = useTaskDataStore.getState();
    // Overflow the cache with an initial page, then rotate away from it.
    store.setTaskCursorPage(pageTasks("first", 100), 100, null, EMPTY_FILTER_OPTIONS, false);

    // Pin: one selected, one active (downloading) entity from page "first",
    // plus one expanded entity on the *current* page (expandedTaskIds are
    // filtered to the visible set by the page setters, so pin a row that
    // stays visible).
    useTaskUIStore.setState({ selectedIds: ["first-0"], selectedId: "first-0" });
    store.upsertTask(makeTask({ id: "first-2", status: "downloading" }));

    // Rotate through enough distinct pages to force eviction of page "first".
    for (let page = 0; page < 8; page += 1) {
      store.setTaskCursorPage(pageTasks(`r${page}`, 100), 100, null, EMPTY_FILTER_OPTIONS, false);
    }
    // Expand a row on the current (last) page so the expanded pin is exercised
    // on an entity that is also visible.
    useTaskDataStore.setState({ expandedTaskIds: ["r7-3"] });

    const cached = useTaskDataStore.getState().taskById;
    expect(cached["first-0"], "selected entity pinned").toBeDefined();
    expect(cached["r7-3"], "expanded+visible entity pinned").toBeDefined();
    expect(cached["first-2"], "active entity pinned").toBeDefined();
    // Non-pinned entities from the first page are evicted.
    expect(cached["first-50"]).toBeUndefined();
  });

  it("evictTasks drops hard-deleted entities from the cache", () => {
    const store = useTaskDataStore.getState();
    store.setTaskCursorPage(pageTasks("del", 5), 5, null, EMPTY_FILTER_OPTIONS, false);
    expect(useTaskDataStore.getState().taskById["del-2"]).toBeDefined();

    useTaskDataStore.getState().evictTasks(["del-2", "del-4", "missing-id"]);
    const cached = useTaskDataStore.getState().taskById;
    expect(cached["del-2"]).toBeUndefined();
    expect(cached["del-4"]).toBeUndefined();
    expect(cached["del-0"]).toBeDefined();
    // View membership is untouched — eviction is an entity-cache concern only.
    expect(useTaskDataStore.getState().taskIds).toHaveLength(5);
  });

  it("keeps cache bounded under append-mode pagination", () => {
    const store = useTaskDataStore.getState();
    // Simulate infinite scroll: eight pages of 100 appended in sequence.
    for (let page = 0; page < 8; page += 1) {
      store.setTaskCursorPage(
        pageTasks(`scroll${page}`, 100),
        800,
        page < 7 ? `cursor-${page}` : null,
        EMPTY_FILTER_OPTIONS,
        page > 0,
      );
    }
    const state = useTaskDataStore.getState();
    expect(state.taskIds).toHaveLength(800);
    const cached = Object.keys(state.taskById).length;
    // All visible rows are pinned, so the cache can exceed the cap only by the
    // pinned set — never by unbounded accumulation.
    expect(cached).toBeGreaterThanOrEqual(800);
    expect(cached).toBeLessThanOrEqual(800 + 500);
  });
});
