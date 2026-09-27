import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useSpeedHistoryStore } from "@/stores/speed-history-store";
import { useTaskDataStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
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

function renderRow(options?: {
  compact?: boolean;
  task?: Task;
  selected?: boolean;
  isFirstFocusable?: boolean;
  segments?: TaskSegment[];
}) {
  const onSelectTask = vi.fn();
  const onShowDetails = vi.fn();
  const onResolveAttention = vi.fn();
  const onOpenFolder = vi.fn();
  const onExtendSelection = vi.fn();
  const onToggleSelected = vi.fn();
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
        selected={options?.selected ?? false}
        multiSelected={false}
        isShiftAnchor={false}
        isFirstFocusable={options?.isFirstFocusable ?? true}
        reduceMotion
        position={1}
        setSize={1}
        onSelectTask={onSelectTask}
        onToggleSelected={onToggleSelected}
        onNavigate={noop}
        onExtendSelection={onExtendSelection}
        onToggleTransfer={noop}
        onRetry={noop}
        onFinishLiveRecording={noop}
        onOpenFile={noop}
        onOpenFolder={onOpenFolder}
        onDelete={noop}
        onResolveAttention={onResolveAttention}
        onShowDetails={onShowDetails}
        segments={options?.segments}
        compact={options?.compact ?? false}
      />
    </TooltipProvider>,
  );

  return { onSelectTask, onShowDetails, onResolveAttention, onOpenFolder, onExtendSelection, onToggleSelected };
}

function segment(overrides: Partial<TaskSegment> & Pick<TaskSegment, "id" | "rangeStart" | "rangeEnd">): TaskSegment {
  return {
    taskId: "task-1",
    fileId: null,
    unitKind: "http_range",
    downloadedUntil: overrides.rangeStart,
    speedBps: 0,
    status: "downloading",
    retryCount: 0,
    lastError: null,
    ...overrides,
  };
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
    expect(onSelectTask).toHaveBeenCalledWith("task-1", "pointer");
    expect(onShowDetails).not.toHaveBeenCalled();

    fireEvent.keyDown(row, { key: "Enter" });
    expect(onShowDetails).toHaveBeenCalledTimes(1);
  });

  it("opens details on double-click for unfinished work", () => {
    // Completed rows keep the Explorer convention (open the file); anything
    // still in flight lands on the evidence instead.
    const { onShowDetails } = renderRow();
    fireEvent.doubleClick(screen.getByRole("listitem"));
    expect(onShowDetails).toHaveBeenCalledTimes(1);
  });

  it("keeps a single route to details: the row button, not a second copy in More", () => {
    renderRow();
    const detailsButtons = screen.getAllByRole("button", { name: "actions.showDetailsFor" });

    fireEvent.click(screen.getAllByRole("button", { name: "actions.moreFor" })[0]);

    expect(screen.getByRole("button", { name: "actions.openFolder" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "actions.showDetailsFor" })).toHaveLength(detailsButtons.length);
    expect(screen.queryByRole("button", { name: "contextmenu.task.showDetails" })).not.toBeInTheDocument();
  });

  it("keeps the canonical details action visible in the row", () => {
    const { onShowDetails } = renderRow();

    fireEvent.click(screen.getAllByRole("button", { name: "actions.showDetailsFor" })[0]);

    expect(onShowDetails).toHaveBeenCalledTimes(1);
  });

  it("mounts both action surfaces so CSS alone owns breakpoint visibility", () => {
    // The JS resize tier used to pick which branch rendered, so a missed resize
    // event could strip every row action. Both branches now stay mounted —
    // the rail is `hidden md:grid`, the stacked row `flex md:hidden` — so
    // exactly one is visible at any width and pause is always reachable.
    renderRow();

    expect(screen.getAllByRole("button", { name: "actions.pauseFor" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "actions.showDetailsFor" })).toHaveLength(2);
    // The in-place expansion is gone: the details panel is the one way in.
    expect(screen.queryByRole("button", { name: "actions.expandFor" })).not.toBeInTheDocument();
  });

  it("keeps only the focused row's controls in the Tab order", () => {
    // Crossing ten rows used to take ~50 Tab presses; arrows move between rows.
    renderRow({ selected: false, isFirstFocusable: false });
    for (const button of screen.getAllByRole("button", { name: "actions.pauseFor" })) {
      expect(button).toHaveAttribute("tabindex", "-1");
    }
    expect(screen.getByRole("checkbox")).toHaveAttribute("tabindex", "-1");
  });

  it("extends the selection with Shift+Arrow and toggles it with Ctrl+Space", () => {
    const { onExtendSelection, onToggleSelected } = renderRow();
    const row = screen.getByRole("listitem");

    fireEvent.keyDown(row, { key: "ArrowDown", shiftKey: true });
    expect(onExtendSelection).toHaveBeenCalledWith("next");
    fireEvent.keyDown(row, { key: " ", ctrlKey: true });
    expect(onToggleSelected).toHaveBeenCalledWith("task-1", true);
  });
});

describe("TaskRow status truth", () => {
  beforeEach(() => {
    useSpeedHistoryStore.setState({ history: {} });
  });

  it("gives retrying its own warning badge and a reconnecting line", () => {
    renderRow({ task: { ...makeTask(), status: "retrying" } });

    expect(document.getElementById("task-task-1-status")?.className).toContain("text-status-warning");
    expect(document.getElementById("task-task-1-diagnostic")).toHaveTextContent("task.diagnostic.retrying");
    expect(document.querySelector(".animate-spin")).toBeNull();
  });

  it("says it is measuring, not waiting, while a fresh transfer has no trend yet", () => {
    renderRow({ task: { ...makeTask(), speedBps: 4096 } });

    expect(document.getElementById("task-task-1-diagnostic")).toHaveTextContent("taskDiagnostics.measuring");
  });

  it("states whether the bytes on disk survive a pause", () => {
    renderRow();
    expect(document.getElementById("task-task-1-host")).toHaveTextContent("task.trust.resumable");
  });

  it("warns before a pause that would discard progress", () => {
    renderRow({ task: { ...makeTask(), supportsResume: false } });

    const mark = screen.getByText("task.trust.pauseRestarts");
    expect(mark).toHaveClass("text-status-warning");
  });

  it("draws the bar as the byte ranges when the list supplies them", () => {
    renderRow({
      task: { ...makeTask(), totalSize: 1000, downloadedBytes: 400 },
      segments: [
        segment({ id: "a", rangeStart: 0, rangeEnd: 499, downloadedUntil: 300, speedBps: 10 }),
        segment({ id: "b", rangeStart: 500, rangeEnd: 999, downloadedUntil: 600, speedBps: 10 }),
      ],
    });

    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("data-chunk-bar");
    expect(bar.children).toHaveLength(2);
    expect(bar).toHaveAttribute("aria-valuenow", "40");
  });

  it("keeps the chunk summary truthful while the task is still downloading", () => {
    renderRow({
      task: { ...makeTask(), totalSize: 1000, downloadedBytes: 1000, status: "downloading" },
      segments: [
        segment({ id: "a", rangeStart: 0, rangeEnd: 499, downloadedUntil: 500, status: "completed" }),
        segment({ id: "b", rangeStart: 500, rangeEnd: 999, downloadedUntil: 1000, status: "completed" }),
      ],
    });

    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      expect.stringContaining("task.status.downloading"),
    );
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

  it("keeps source and diagnostic context visible in compact density", () => {
    // The row's aria-describedby points at these ids, so compact mode keeps
    // the same source and diagnostic nodes available to assistive technology.
    renderRow({ compact: true });
    const row = screen.getByRole("listitem");

    expect(row).toHaveAttribute("aria-describedby", expect.stringContaining("task-task-1-host"));
    expect(document.getElementById("task-task-1-host")).not.toHaveClass("sr-only");
    expect(document.getElementById("task-task-1-diagnostic")).not.toHaveClass("sr-only");
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

describe("TaskRow completion progress display", () => {
  it("shows a finishing state while all bytes are present but the task is active", () => {
    renderRow({
      task: {
        ...makeTask(),
        downloadedBytes: 100,
        speedBps: 0,
        healthSummary: "taskDiagnostics.downloading",
      },
    });

    expect(screen.getByText("task.status.finishing")).toBeInTheDocument();
    expect(screen.getByText("task.diagnostic.finishing")).toBeInTheDocument();
    expect(screen.queryByText("taskDiagnostics.downloading")).not.toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "100");
  });

  it("keeps an unfinished transfer below 100 when display rounding would complete it", () => {
    renderRow({ task: { ...makeTask(), totalSize: 10_000, downloadedBytes: 9_999 } });

    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "99");
    expect(screen.queryByText("task.status.finishing")).not.toBeInTheDocument();
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

  it("keeps file-location access in More instead of duplicating it in recovery", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    const { onResolveAttention, onOpenFolder } = renderRow({ task: failed });

    // The banner's second line explains why, not just what happened.
    expect(screen.getByText("errors.cause.resumeUnavailable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "recovery.restart" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "recovery.open_folder" })).not.toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "actions.moreFor" })[0]);
    fireEvent.click(screen.getByRole("button", { name: "actions.openFolder" }));

    expect(onOpenFolder).toHaveBeenCalledWith(failed);
    expect(onResolveAttention).not.toHaveBeenCalled();
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

  it("tints needs-attention amber and shows the restart cost", () => {
    const attention = {
      ...makeTask(),
      status: "needs_attention" as const,
      errorMessage: "Remote changed",
      errorCode: "remote_changed",
      recoveryActions: ["restart", "open_folder"] as Task["recoveryActions"],
      speedBps: 0,
      downloadedBytes: 512,
    };

    renderRow({ task: attention });

    // Waiting on a decision is amber, the same as the sidebar and both
    // centers; red stays reserved for failed.
    expect(document.getElementById("task-task-1-status")?.className).toContain("text-status-warning");
    expect(screen.getByRole("button", { name: "recovery.restart" })).toHaveAttribute("title", "recovery.restartCost");
    expect(screen.getByText("recovery.restartCost")).toBeInTheDocument();
  });

  it("opens the remaining fixes in place when two or more are hidden", () => {
    const failed = {
      ...makeTask(),
      status: "failed" as const,
      errorMessage: "Resume unavailable",
      errorCode: "resume_unavailable",
      recoveryActions: ["restart", "choose_another_name", "choose_another_folder"] as Task["recoveryActions"],
      speedBps: 0,
    };

    const { onResolveAttention } = renderRow({ task: failed });

    const moreFixes = screen.getByRole("button", { name: "actions.moreFixesTitle" });
    expect(moreFixes).toHaveAttribute("aria-expanded", "false");
    expect(moreFixes).toHaveTextContent("actions.moreFixesCount");
    expect(screen.queryByRole("button", { name: "recovery.choose_another_name" })).not.toBeInTheDocument();

    // It used to expand the row, which showed the save folder, not the fixes.
    fireEvent.click(moreFixes);
    fireEvent.click(screen.getByRole("button", { name: "recovery.choose_another_name" }));
    expect(onResolveAttention).toHaveBeenCalledWith(failed, "choose_another_name");
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

  it("replaces a health summary that only repeats the badge with the next useful fact", () => {
    renderRow({
      task: { ...makeTask(), status: "completed", speedBps: 0, healthSummary: "task.status.completed" },
    });

    expect(document.getElementById("task-task-1-diagnostic")).toHaveTextContent("task.diagnostic.completedAt");
  });

  // Regression: "Waiting for network" rows showed no reason line because the
  // backend summary equalled the badge and suppressed the fallback.
  it("tells a waiting-for-network row that it will resume on its own", () => {
    renderRow({
      task: { ...makeTask(), status: "waiting_network", speedBps: 0, healthSummary: "task.status.waiting_network" },
    });

    expect(document.getElementById("task-task-1-diagnostic")).toHaveTextContent("task.diagnostic.waitingNetwork");
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
