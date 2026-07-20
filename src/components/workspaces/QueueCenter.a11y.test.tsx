import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { QueueCenter } from "./QueueCenter";

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

vi.mock("@/lib/tauri", () => ({
  getSchedulerSnapshot: vi.fn(async () => ({
    activeTaskCount: 0,
    maxActiveTasks: 2,
    scheduleWindowEnabled: false,
    scheduleWindowActive: true,
    scheduleWindowStart: "00:00",
    scheduleWindowEnd: "23:59",
    decisions: [],
  })),
}));

function makeTask(id: string, fileName: string, priority: Task["priority"] = "normal"): Task {
  return {
    id,
    url: `https://example.com/${id}`,
    finalUrl: `https://example.com/${id}`,
    protocol: "https",
    taskKind: "single_file",
    fileName,
    saveDir: "C:/downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 1024,
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
    priority,
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
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    files: [],
  };
}

describe("QueueCenter a11y (UX-10)", () => {
  beforeEach(() => {
    const tasks = [makeTask("a", "a.zip", "high"), makeTask("b", "b.zip"), makeTask("c", "c.zip", "low")];
    useTaskDataStore.setState({
      taskById: Object.fromEntries(tasks.map((task) => [task.id, task])),
      taskIds: tasks.map((task) => task.id),
      tasks,
    } as never);
    useTaskUIStore.setState({ selectedId: "a" } as never);
  });

  it("uses a semantic list with a single tab stop and arrow-key navigation", () => {
    render(
      <TooltipProvider>
        <QueueCenter
          taskIds={["a", "b", "c"]}
          loading={false}
          error={null}
          hasMore={false}
          onLoadMore={() => undefined}
          onRetryLoad={() => undefined}
          onPause={() => undefined}
          onUpdateOptions={async () => true}
        />
      </TooltipProvider>,
    );

    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items.filter((item) => item.tabIndex === 0)).toHaveLength(1);
    expect(items[0]).toHaveAttribute("tabindex", "0");

    fireEvent.keyDown(items[0]!, { key: "ArrowDown" });
    expect(useTaskUIStore.getState().selectedId).toBe("b");

    fireEvent.keyDown(document.getElementById("queue-task-b")!, { key: "End" });
    expect(useTaskUIStore.getState().selectedId).toBe("c");

    fireEvent.keyDown(document.getElementById("queue-task-c")!, { key: "Home" });
    expect(useTaskUIStore.getState().selectedId).toBe("a");
  });
});
