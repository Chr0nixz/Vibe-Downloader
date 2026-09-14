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
  const onResolveAttention = vi.fn();
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
        onResolveAttention={onResolveAttention}
        onShowDetails={onShowDetails}
        compact={options?.compact ?? false}
      />
    </TooltipProvider>,
  );

  return { onSelectTask, onShowDetails, onResolveAttention };
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

  it("mounts both action surfaces so CSS alone owns breakpoint visibility", () => {
    // The JS resize tier used to pick which branch rendered, so a missed resize
    // event could strip every row action. Both branches now stay mounted —
    // the rail is `hidden md:grid`, the stacked row `flex md:hidden` — so
    // exactly one is visible at any width and pause is always reachable.
    renderRow();

    expect(screen.getAllByRole("button", { name: "actions.pauseFor" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "actions.expandFor" })).toHaveLength(2);
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

  it("hands an explicit error message to the recovery banner instead of the diagnostic line", () => {
    renderRow({
      task: { ...makeTask(), status: "failed", speedBps: 0, errorMessage: "Resume unavailable" },
    });

    // The banner carries message + cause; a duplicate diagnostic line would
    // stack the same words twice on one row.
    expect(diagnosticLine()).toBeNull();
    expect(screen.getByText("errors.resumeUnavailable")).toBeInTheDocument();
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

  it("surfaces the error cause and names the single alternative fix on its face", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    const { onResolveAttention } = renderRow({ task: failed });

    // The banner's second line explains why, not just what happened.
    expect(screen.getByText("errors.cause.resumeUnavailable")).toBeInTheDocument();
    // One hidden fix used to hide behind a count + hover tooltip, so the safer
    // branch had to be memorized before choosing it over a destructive restart.
    // A single alternative is now a named button that resolves directly.
    fireEvent.click(screen.getByRole("button", { name: "recovery.open_folder" }));
    expect(onResolveAttention).toHaveBeenCalledWith(failed, "open_folder");
  });

  it("renders the banner restart with the danger tint the expanded view uses", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    renderRow({ task: failed });

    // Restart discards downloaded bytes; the banner must not dress it in the
    // accent variant that marks the recommended action.
    expect(screen.getByRole("button", { name: "recovery.restart" })).toHaveClass("bg-status-danger/15");
  });

  it("keeps the count + expand affordance when two or more fixes are hidden", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "choose_another_name", "choose_another_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    renderRow({ task: failed });

    const moreFixes = screen.getByRole("button", { name: "actions.moreFixesTitle" });
    expect(moreFixes).toHaveAttribute("aria-expanded", "false");
    expect(moreFixes).toHaveAttribute("aria-controls", "task-task-1-expanded");
    expect(moreFixes).toHaveTextContent("actions.moreFixesCount");
    expect(screen.queryByRole("button", { name: "recovery.choose_another_name" })).not.toBeInTheDocument();
  });
});

describe("TaskRow terminal-row density", () => {
  beforeEach(() => {
    useSpeedHistoryStore.setState({ history: {} });
  });

  const rail = () => screen.getByRole("listitem");

  it("drops the speed slot from the rail when no transfer is running", () => {
    // The em-dash placeholder used to own the rail's most prominent slot on
    // every inactive row; the progress line takes that slot instead.
    renderRow({ task: { ...makeTask(), status: "paused", speedBps: 0 } });

    expect(rail().querySelectorAll('[data-slot="speed"]')).toHaveLength(0);
    expect(rail().querySelectorAll('[data-slot="progress"]')).toHaveLength(1);
  });

  it("drops the bar and states the size once on completed rows", () => {
    // A pinned 100% bar plus "X / X" bytes plus a 100% label repeated one fact
    // three times on the list's most common row type; completed rows now keep
    // a single measurement line.
    renderRow({ task: { ...makeTask(), status: "completed", downloadedBytes: 100, speedBps: 0 } });

    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(rail().querySelectorAll('[data-slot="size"]')).toHaveLength(1);
    expect(rail().querySelector('[data-slot="size"]')).toHaveTextContent("100 B");
    expect(rail().querySelectorAll('[data-slot="bytes"]')).toHaveLength(0);
    expect(rail().querySelectorAll('[data-slot="progress"]')).toHaveLength(0);
  });

  it("renders a stalled transfer as 0 KB/s with a warning tone", () => {
    // "—" merged "dead" and "stalled"; an active transfer at 0 B/s is a fact.
    renderRow({ task: { ...makeTask(), status: "downloading", speedBps: 0 } });

    const speed = rail().querySelector('[data-slot="speed"]');
    expect(speed).toHaveTextContent("0 KB/s");
    expect(speed).toHaveClass("text-status-warning");
  });

  it("hides the diagnostic line when it only repeats the badge", () => {
    renderRow({
      task: { ...makeTask(), status: "completed", speedBps: 0, healthSummary: "task.status.completed" },
    });

    expect(document.getElementById("task-task-1-diagnostic")).toBeNull();
    expect(rail()).toHaveAttribute("aria-describedby", expect.not.stringContaining("-diagnostic"));
  });

  it("keeps the checksum verdict line on completed rows", () => {
    renderRow({
      task: { ...makeTask(), status: "completed", speedBps: 0, hashStatus: "verified" },
    });

    expect(document.getElementById("task-task-1-diagnostic")).toHaveTextContent("task.diagnostic.checksumVerified");
  });

  it("hides the diagnostic line when the recovery banner already shows the message", () => {
    renderRow({
      task: {
        ...makeTask(),
        status: "failed",
        speedBps: 0,
        errorMessage: "Resume unavailable",
        errorCode: "resume_unavailable",
        recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      },
    });

    expect(document.getElementById("task-task-1-diagnostic")).toBeNull();
    expect(screen.getByText("errors.cause.resumeUnavailable")).toBeInTheDocument();
  });
});
