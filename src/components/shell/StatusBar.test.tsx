import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { EMPTY_TASK_STATS } from "@/stores/task-data-store";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { StatusBar } from "./StatusBar";

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

// The updater and speed-limit popover talk to Tauri; neither is under test.
vi.mock("@/hooks/use-app-updater", () => ({
  useAppUpdater: () => ({
    updateVersion: null,
    installing: false,
    error: null,
    installUpdate: vi.fn(),
    dismissUpdate: vi.fn(),
    checkForUpdate: vi.fn(),
  }),
}));

vi.mock("@/components/shell/SpeedLimitControl", () => ({
  SpeedLimitControl: () => null,
}));

function renderBar() {
  return render(
    <TooltipProvider>
      <StatusBar />
    </TooltipProvider>,
  );
}

describe("StatusBar health", () => {
  beforeEach(() => {
    useTaskUIStore.setState({ nav: "all" } as never);
  });

  it("stays quiet when nothing is stuck", () => {
    useTaskDataStore.setState({ globalTaskStats: { ...EMPTY_TASK_STATS } } as never);
    renderBar();

    expect(screen.getByText("statusBar.idle")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "statusBar.attentionCount" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "statusBar.failedCount" })).not.toBeInTheDocument();
  });

  it("surfaces stuck tasks beside the idle label and leads to them", () => {
    // "No active downloads" alone used to read as all-clear while two tasks
    // were waiting on the user.
    useTaskDataStore.setState({ globalTaskStats: { ...EMPTY_TASK_STATS, attention: 2, failed: 1 } } as never);
    renderBar();

    expect(screen.getByText("statusBar.idle")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "statusBar.attentionCount" }));
    expect(useTaskUIStore.getState().nav).toBe("attention");

    fireEvent.click(screen.getByRole("button", { name: "statusBar.failedCount" }));
    expect(useTaskUIStore.getState().nav).toBe("failed");
  });

  it("reads as one activity sentence instead of a strip of counters", () => {
    useTaskDataStore.setState({
      globalTaskStats: { ...EMPTY_TASK_STATS, active: 2, queued: 3, totalSpeed: 6_900_000 },
    } as never);
    renderBar();

    expect(screen.getByText("statusBar.downloadingAt")).toBeInTheDocument();
    expect(screen.queryByText("statusBar.idle")).not.toBeInTheDocument();
    // Queued work waits in the summary; the bar itself no longer lists it.
    expect(screen.queryByText("3")).not.toBeInTheDocument();
  });

  it("says what is queued when nothing is downloading", () => {
    useTaskDataStore.setState({ globalTaskStats: { ...EMPTY_TASK_STATS, queued: 3 } } as never);
    renderBar();

    expect(screen.getByText("statusBar.queuedOnly")).toBeInTheDocument();
  });

  it("keeps paused, waiting, and completed work one click away in the summary", () => {
    useTaskDataStore.setState({
      globalTaskStats: { ...EMPTY_TASK_STATS, paused: 2, waitingNetwork: 1, completed: 4 },
    } as never);
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "statusBar.summary" }));
    const summary = screen.getByRole("list", { name: "statusBar.summary" });
    // Waiting for network has no view of its own, so it is shown, not linked.
    expect(within(summary).getByText("task.status.waiting_network").closest("button")).toBeNull();

    fireEvent.click(within(summary).getByRole("button", { name: /nav\.paused\s*2/ }));
    expect(useTaskUIStore.getState().nav).toBe("paused");

    fireEvent.click(screen.getByRole("button", { name: "statusBar.summary" }));
    fireEvent.click(
      within(screen.getByRole("list", { name: "statusBar.summary" })).getByRole("button", {
        name: /nav\.completed\s*4/,
      }),
    );
    expect(useTaskUIStore.getState().nav).toBe("completed");
  });
});
