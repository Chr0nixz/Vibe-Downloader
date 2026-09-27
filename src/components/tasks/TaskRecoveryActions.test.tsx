import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { TFunction } from "i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RecoveryAction } from "@/generated/bindings";
import { formatBytes } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";
import { restartCost, TaskRecoveryActions } from "./TaskRecoveryActions";

// Mock react-i18next: keep original exports (initReactI18next etc.) but
// override useTranslation to return the key as-is for assertions.
vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
    }),
  };
});

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
    status: "failed",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "manual",
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

describe("TaskRecoveryActions", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useToastStore.setState({ toasts: [] });
  });

  it("renders nothing when there is no error message", () => {
    const task = makeTask({ errorMessage: null });
    const onResolve = vi.fn();
    const { container } = render(<TaskRecoveryActions task={task} onResolve={onResolve} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders recovery action buttons when task has error and recovery actions", () => {
    const task = makeTask({
      errorMessage: "disk_write_failed: No space left on device",
      recoveryActions: ["choose_another_folder", "free_disk_space"] as RecoveryAction[],
    });
    const onResolve = vi.fn();
    render(<TaskRecoveryActions task={task} onResolve={onResolve} />);

    // The group opens with the shared problem statement: the concern in the
    // Recovery Center's vocabulary, then the message. No live region — it
    // re-rendered for every task arrowed through with details open.
    expect(screen.getByRole("group")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "recoveryCenter.concern.disk" })).toBeInTheDocument();

    // Both recovery action buttons must be rendered with their i18n keys.
    expect(screen.getByText("recovery.choose_another_folder")).toBeInTheDocument();
    expect(screen.getByText("recovery.free_disk_space")).toBeInTheDocument();
  });

  it("tints needs-attention amber and failed red", () => {
    const actions = ["free_disk_space"] as RecoveryAction[];
    const { rerender } = render(
      <TaskRecoveryActions
        task={makeTask({ status: "needs_attention", errorMessage: "disk full", recoveryActions: actions })}
        onResolve={vi.fn()}
      />,
    );
    expect(screen.getByRole("group").className).toContain("border-border-warning-subtle");

    rerender(
      <TaskRecoveryActions
        task={makeTask({ status: "failed", errorMessage: "disk full", recoveryActions: actions })}
        onResolve={vi.fn()}
      />,
    );
    expect(screen.getByRole("group").className).toContain("border-border-danger-subtle");
  });

  it("states the restart price beside the buttons in both variants", () => {
    const task = makeTask({
      status: "needs_attention",
      errorMessage: "remote changed",
      recoveryActions: ["restart", "open_folder"] as RecoveryAction[],
      downloadedBytes: 2 * 1024 ** 3,
      totalSize: 7.5 * 1024 ** 3,
    });
    const { rerender } = render(<TaskRecoveryActions task={task} onResolve={vi.fn()} />);
    expect(screen.getByText("recovery.restartCost")).toBeInTheDocument();

    rerender(<TaskRecoveryActions task={task} onResolve={vi.fn()} showMessage={false} />);
    expect(screen.getByText("recovery.restartCost")).toBeInTheDocument();

    // Nothing on disk yet: no price to state.
    rerender(<TaskRecoveryActions task={{ ...task, downloadedBytes: 0 }} onResolve={vi.fn()} />);
    expect(screen.queryByText("recovery.restartCost")).not.toBeInTheDocument();
  });

  it("calls onResolve with the task and action when a button is clicked", () => {
    const task = makeTask({
      errorMessage: "disk_write_failed: No space left on device",
      recoveryActions: ["free_disk_space"] as RecoveryAction[],
    });
    const onResolve = vi.fn();
    render(<TaskRecoveryActions task={task} onResolve={onResolve} />);

    const button = screen.getByText("recovery.free_disk_space");
    fireEvent.click(button);

    expect(onResolve).toHaveBeenCalledTimes(1);
    expect(onResolve).toHaveBeenCalledWith(task, "free_disk_space");
  });

  it("renders a copy-error button alongside the error message", () => {
    const task = makeTask({
      errorMessage: "disk_write_failed: No space left on device",
      recoveryActions: ["free_disk_space"] as RecoveryAction[],
    });
    const onResolve = vi.fn();
    render(<TaskRecoveryActions task={task} onResolve={onResolve} />);

    // The copy-error button has an accessible label from the i18n key.
    const copyButton = screen.getByLabelText("recovery.copyError");
    expect(copyButton).toBeInTheDocument();
  });

  it("toasts copy success only after the clipboard write resolves (UX-24)", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    const task = makeTask({
      errorMessage: "disk_write_failed: No space left on device",
      recoveryActions: ["free_disk_space"] as RecoveryAction[],
    });
    render(<TaskRecoveryActions task={task} onResolve={vi.fn()} />);

    fireEvent.click(screen.getByLabelText("recovery.copyError"));
    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0].title).toBe("recovery.errorCopied");
    });
  });

  it("prices a restart in bytes, and says nothing when there is nothing to lose", () => {
    const t = ((key: string, options?: Record<string, unknown>) => ({ key, ...options })) as unknown as TFunction;
    const downloaded = 2 * 1024 ** 3;
    const total = 7.5 * 1024 ** 3;

    expect(restartCost({ downloadedBytes: downloaded, totalSize: total }, t)).toEqual({
      key: "recovery.restartCost",
      downloaded: formatBytes(downloaded),
      total: formatBytes(total),
    });
    // Unknown-size downloads cannot promise a re-download total.
    expect(restartCost({ downloadedBytes: downloaded, totalSize: 0 }, t)).toEqual({
      key: "recovery.restartCostUnknownTotal",
      downloaded: formatBytes(downloaded),
    });
    expect(restartCost({ downloadedBytes: 0, totalSize: total }, t)).toBeNull();
  });

  it("toasts copy failure instead of swallowing clipboard errors (UX-24)", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("clipboard blocked"));
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    const task = makeTask({
      errorMessage: "disk_write_failed: No space left on device",
      recoveryActions: ["free_disk_space"] as RecoveryAction[],
    });
    render(<TaskRecoveryActions task={task} onResolve={vi.fn()} />);

    fireEvent.click(screen.getByLabelText("recovery.copyError"));
    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts).toHaveLength(1);
      expect(toasts[0].tone).toBe("error");
      expect(toasts[0].title).toBe("contextmenu.task.copyFailed");
    });
  });
});
