import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { AttentionCenter } from "./AttentionCenter";

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

function makeAttentionTask(id: string): Task {
  return {
    id,
    url: `https://example.com/${id}`,
    finalUrl: `https://example.com/${id}`,
    protocol: "https",
    taskKind: "single_file",
    fileName: `${id}.zip`,
    saveDir: "C:/downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 1024,
    downloadedBytes: 512,
    status: "needs_attention",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 1,
    speedBps: 0,
    taskSpeedLimitBps: null,
    priority: "normal",
    queuePosition: "0",
    categoryKey: null,
    obeySchedule: true,
    healthSummary: null,
    errorMessage: "remote_changed: Remote file changed",
    errorCode: "remote_changed",
    recoveryActions: ["restart", "check_url"],
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

describe("AttentionCenter a11y (UX-18)", () => {
  beforeEach(() => {
    const tasks = [makeAttentionTask("a"), makeAttentionTask("b")];
    useTaskDataStore.setState({
      taskById: Object.fromEntries(tasks.map((task) => [task.id, task])),
      taskIds: tasks.map((task) => task.id),
      tasks,
    } as never);
    useTaskUIStore.setState({ selectedId: "a" } as never);
  });

  it("uses the shared list/listitem semantics instead of listbox/option (UX-18)", () => {
    render(
      <TooltipProvider>
        <AttentionCenter
          taskIds={["a", "b"]}
          loading={false}
          error={null}
          hasMore={false}
          onLoadMore={() => undefined}
          onRetryLoad={() => undefined}
          onResolve={() => undefined}
        />
      </TooltipProvider>,
    );

    // UX-18: TaskList and QueueCenter already use list/listitem +
    // aria-current; AttentionCenter was the only listbox/option holdout.
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveAttribute("aria-current", "true");
    expect(items[1]).not.toHaveAttribute("aria-current");
    // One tab stop, moved with the selection — same model as QueueCenter.
    expect(items.filter((item) => item.tabIndex === 0)).toHaveLength(1);
  });

  it("keeps arrow-key navigation working on the list container", () => {
    render(
      <TooltipProvider>
        <AttentionCenter
          taskIds={["a", "b"]}
          loading={false}
          error={null}
          hasMore={false}
          onLoadMore={() => undefined}
          onRetryLoad={() => undefined}
          onResolve={() => undefined}
        />
      </TooltipProvider>,
    );

    const firstRow = document.getElementById("attention-task-a")!;
    fireEvent.keyDown(firstRow, { key: "ArrowDown" });
    expect(useTaskUIStore.getState().selectedId).toBe("b");
  });
});
