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

function renderRow(options?: { compact?: boolean; task?: Task }) {
  const onSelectTask = vi.fn();
  const onShowDetails = vi.fn();
  const noop = vi.fn();
  const task = options?.task ?? makeTask();

  useTaskDataStore.setState({
    taskIds: [task.id],
    taskById: { [task.id]: task },
    expandedTaskIds: [],
    completionFlashIds: [],
  });

  render(
    <TooltipProvider>
      <TaskRow
        taskId={task.id}
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
        shellCompact={false}
        compact={options?.compact ?? false}
      />
    </TooltipProvider>,
  );

  return { onSelectTask, onShowDetails };
}

describe("TaskRow interaction semantics", () => {
  beforeEach(() => {
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

describe("TaskRow diagnostic line", () => {
  beforeEach(() => {
    useSpeedHistoryStore.setState({ history: {} });
  });

  const diagnosticLine = () => document.querySelector('[id$="-diagnostic"]');

  // The badge already names the state, so the row's one free-form text slot has
  // to carry the *next* useful fact instead of repeating it.
  it.each([
    ["paused", "task.diagnostic.pausedAt"],
    ["queued", "task.diagnostic.queuedWaiting"],
    ["waiting_network", "task.diagnostic.waitingNetwork"],
    ["completed", "task.diagnostic.completedAt"],
  ] as const)("reports a fact other than the badge for a %s task", (status, expectedKey) => {
    renderRow({ task: { ...makeTask(), status, speedBps: 0 } });

    expect(diagnosticLine()).toHaveTextContent(expectedKey);
    expect(diagnosticLine()).not.toHaveTextContent(`task.status.${status}`);
  });

  it("prefers the checksum verdict over the completion time once a hash exists", () => {
    renderRow({
      task: { ...makeTask(), status: "completed", speedBps: 0, hashStatus: "verified" },
    });

    expect(diagnosticLine()).toHaveTextContent("task.diagnostic.checksumVerified");
  });

  it("lets an explicit error message win over the status fact", () => {
    renderRow({
      task: { ...makeTask(), status: "failed", speedBps: 0, errorMessage: "Resume unavailable" },
    });

    expect(diagnosticLine()).not.toHaveTextContent("task.diagnostic.stoppedAt");
  });
});

describe("TaskRow compact density", () => {
  beforeEach(() => {
    useSpeedHistoryStore.setState({ history: {} });
  });

  it("keeps the hidden host and diagnostic lines in the accessibility tree", () => {
    // The row's aria-describedby points at these ids, so compact mode has to
    // sr-only them rather than unmount them.
    renderRow({ compact: true });
    const row = screen.getByRole("listitem");

    expect(row).toHaveAttribute("aria-describedby", expect.stringContaining("task-task-1-host"));
    expect(document.getElementById("task-task-1-host")).toHaveClass("sr-only");
    expect(document.getElementById("task-task-1-diagnostic")).toHaveClass("sr-only");
  });

  it("folds the byte count into a tooltip and keeps two rail lines", () => {
    renderRow({ compact: true });
    const rail = screen.getByRole("listitem");

    expect(rail.querySelectorAll('[data-slot="speed"]')).toHaveLength(1);
    expect(rail.querySelectorAll('[data-slot="progress"]')).toHaveLength(1);
    expect(rail.querySelector('[data-slot="bytes"]')).toBeNull();
    expect(rail.querySelector('[data-slot="progress"]')).toHaveAttribute("title", "25 B / 100 B");
  });

  it("rides the connection count on the progress line instead of a rail row", () => {
    renderRow({ compact: true });
    const rail = screen.getByRole("listitem");

    expect(rail.querySelectorAll('[data-slot="connections"]')).toHaveLength(0);
    expect(rail.querySelector('[data-slot="progress"]')).toHaveTextContent("task.connections");
  });
});

describe("TaskRow recovery actions", () => {
  beforeEach(() => {
    useSpeedHistoryStore.setState({ history: {} });
  });

  it("keeps a single restart control when resume is unavailable", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    renderRow({ task: failed });

    expect(screen.getByRole("button", { name: "recovery.restart" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "actions.resumeFor" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "actions.retryFor" })).not.toBeInTheDocument();
  });
});
