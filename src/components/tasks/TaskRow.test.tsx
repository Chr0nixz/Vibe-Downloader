import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useSpeedHistoryStore } from "@/stores/speed-history-store";
import { useTaskDataStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { TaskRow } from "./TaskRow";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
    }),
  };
});

vi.mock("@/components/tasks/TaskContextMenu", () => ({
  TaskContextMenu: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/hooks/use-shell-layout", () => ({
  useShellLayout: () => "wide",
}));

vi.mock("@/hooks/use-system-file-icon", () => ({
  useSystemFileIcon: () => null,
}));

function makeTask(): Task {
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
    downloadedBytes: 25,
    status: "downloading",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 4,
    speedBps: 1024,
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
  };
}

function renderRow() {
  const onSelectTask = vi.fn();
  const onShowDetails = vi.fn();
  const noop = vi.fn();

  render(
    <TooltipProvider>
      <TaskRow
        taskId="task-1"
        selected={false}
        multiSelected={false}
        isShiftAnchor={false}
        isFirstFocusable
        reduceMotion
        position={1}
        setSize={1}
        onSelectTask={onSelectTask}
        onToggleSelected={noop}
        onNavigate={noop}
        onToggleTransfer={noop}
        onRetry={noop}
        onFinishLiveRecording={noop}
        onOpenFile={noop}
        onOpenFolder={noop}
        onDelete={noop}
        onResolveAttention={noop}
        onShowDetails={onShowDetails}
      />
    </TooltipProvider>,
  );

  return { onSelectTask, onShowDetails };
}

describe("TaskRow interaction semantics", () => {
  beforeEach(() => {
    const task = makeTask();
    useTaskDataStore.setState({
      taskIds: [task.id],
      taskById: { [task.id]: task },
      expandedTaskIds: [],
      completionFlashIds: [],
    });
    useSpeedHistoryStore.setState({ history: {} });
  });

  it("uses list-item semantics for a row that contains independent controls", () => {
    renderRow();

    expect(screen.getByRole("listitem")).toBeInTheDocument();
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("keeps selection separate from opening details", () => {
    const { onSelectTask, onShowDetails } = renderRow();
    const row = screen.getByRole("listitem");

    fireEvent.click(row);
    expect(onSelectTask).toHaveBeenCalledWith("task-1");
    expect(onShowDetails).not.toHaveBeenCalled();

    fireEvent.keyDown(row, { key: "Enter" });
    expect(onShowDetails).toHaveBeenCalledTimes(1);
  });
});
