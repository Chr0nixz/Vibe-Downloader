import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RecoveryHistoryRecord } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { RecoveryCenter } from "./RecoveryCenter";

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

const bulkResolveAttention = vi.fn();
const updateTaskCredentials = vi.fn();
const listRecoveryHistory = vi.fn();

vi.mock("@/lib/tauri", () => ({
  bulkResolveAttention: (...args: unknown[]) => bulkResolveAttention(...args),
  updateTaskCredentials: (...args: unknown[]) => updateTaskCredentials(...args),
  listRecoveryHistory: (...args: unknown[]) => listRecoveryHistory(...args),
}));

vi.mock("@/lib/format-date", () => ({
  formatDateTime: () => "2026-09-13 12:00",
}));

import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";

function makeTask(overrides: Partial<Task> & { id: string }): Task {
  return {
    url: `https://example.com/${overrides.id}`,
    finalUrl: `https://example.com/${overrides.id}`,
    protocol: "http",
    taskKind: "single_file",
    fileName: `${overrides.id}.bin`,
    saveDir: "/tmp",
    tempPath: null,
    finalPath: `/tmp/${overrides.id}.bin`,
    totalSize: 100,
    downloadedBytes: 0,
    status: "failed",
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
    createdAt: "2026-09-13T00:00:00Z",
    updatedAt: "2026-09-13T00:00:00Z",
    files: [],
    ...overrides,
  } as Task;
}

const noop = async () => {};

function renderList() {
  return render(
    <RecoveryCenter
      taskIds={useTaskDataStore.getState().taskIds}
      loading={false}
      error={null}
      hasMore={false}
      onLoadMore={noop}
      onRetryLoad={noop}
      onResolve={noop}
    />,
  );
}

describe("RecoveryCenter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
      nav: "recovery",
      search: "",
      selectedId: null,
      selectedIds: [],
      pendingDeleteIds: [],
    });
  });

  it("groups failures by concern and shows auto/needs-input counts", () => {
    const failed = makeTask({ id: "http-1", failureCategory: "http", status: "failed" });
    const changed = makeTask({
      id: "changed-1",
      failureCategory: "remote_changed",
      errorCode: "remote_changed",
      status: "needs_attention",
      recoveryActions: ["restart", "check_url"],
    });
    useTaskDataStore.setState({
      tasks: [failed, changed],
      taskIds: [failed.id, changed.id],
      taskById: { [failed.id]: failed, [changed.id]: changed },
    });

    renderList();

    // The concern labels appear in the group headers and again in the
    // auto-selected detail pane, hence getAllByText.
    expect(screen.getAllByText("recoveryCenter.concern.http").length).toBeGreaterThan(0);
    expect(screen.getAllByText("recoveryCenter.concern.remoteChanged").length).toBeGreaterThan(0);
    expect(screen.getByText("recoveryCenter.row.auto")).toBeInTheDocument();
    expect(screen.getByText("recoveryCenter.row.needsInput")).toBeInTheDocument();
  });

  it("retries a group through the backend with the auto-retryable ids only", async () => {
    const failed = makeTask({ id: "http-1", failureCategory: "http", status: "failed" });
    const changed = makeTask({
      id: "changed-1",
      failureCategory: "remote_changed",
      errorCode: "remote_changed",
      status: "needs_attention",
      recoveryActions: ["restart"],
    });
    useTaskDataStore.setState({
      tasks: [failed, changed],
      taskIds: [failed.id, changed.id],
      taskById: { [failed.id]: failed, [changed.id]: changed },
    });
    bulkResolveAttention.mockResolvedValue({ succeeded: 1, skipped: 0, failed: 0 });

    renderList();
    // Only the http group has auto-retryable tasks, so it is the only group
    // whose "retry all" button is enabled.
    const retryButtons = screen.getAllByRole("button", { name: "recoveryCenter.group.retryAll" });
    const enabled = retryButtons.filter((button) => !button.hasAttribute("disabled"));
    expect(enabled).toHaveLength(1);
    fireEvent.click(enabled[0]);

    await waitFor(() => expect(bulkResolveAttention).toHaveBeenCalledTimes(1));
    expect(bulkResolveAttention).toHaveBeenCalledWith(["http-1"], "retry");
  });

  it("offers shared credential repair for auth tasks over credential-capable protocols", async () => {
    const authFtp = makeTask({
      id: "auth-1",
      failureCategory: "auth",
      protocol: "ftp",
      status: "failed",
    });
    useTaskDataStore.setState({
      tasks: [authFtp],
      taskIds: [authFtp.id],
      taskById: { [authFtp.id]: authFtp },
    });
    updateTaskCredentials.mockResolvedValue(authFtp);

    renderList();
    fireEvent.click(screen.getByRole("button", { name: "recoveryCenter.credentials.open" }));

    fireEvent.change(screen.getByLabelText("recoveryCenter.credentials.username"), {
      target: { value: "alice" },
    });
    fireEvent.change(screen.getByLabelText("recoveryCenter.credentials.password"), {
      target: { value: "s3cret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "recoveryCenter.credentials.save" }));

    await waitFor(() => expect(updateTaskCredentials).toHaveBeenCalledTimes(1));
    expect(updateTaskCredentials).toHaveBeenCalledWith({
      taskId: "auth-1",
      username: "alice",
      password: "s3cret",
      privateKeyData: null,
      privateKeyPassphrase: null,
    });
  });

  it("renders the recovery history with action and source labels", async () => {
    const failed = makeTask({ id: "http-1", failureCategory: "http", status: "failed" });
    useTaskDataStore.setState({
      tasks: [failed],
      taskIds: [failed.id],
      taskById: { [failed.id]: failed },
    });
    const record: RecoveryHistoryRecord = {
      id: "entry-1",
      taskId: "http-1",
      taskFileName: "http-1.bin",
      action: "retry",
      source: "recovery_center",
      errorCode: "http_status",
      createdAt: "2026-09-13T11:00:00Z",
    };
    listRecoveryHistory.mockResolvedValue([record]);

    renderList();
    fireEvent.click(screen.getByRole("button", { name: "recoveryCenter.history.title" }));

    await waitFor(() => expect(listRecoveryHistory).toHaveBeenCalledWith(30));
    expect(await screen.findByText("http-1.bin")).toBeInTheDocument();
    expect(screen.getByText(/recoveryCenter.history.action.retry/)).toBeInTheDocument();
    expect(screen.getByText(/recoveryCenter.history.source.recovery_center/)).toBeInTheDocument();
  });

  it("renders the playbook in the detail pane with consequence lines", () => {
    const changed = makeTask({
      id: "changed-1",
      failureCategory: "remote_changed",
      errorCode: "remote_changed",
      status: "needs_attention",
      recoveryActions: ["restart", "check_url"],
    });
    useTaskDataStore.setState({
      tasks: [changed],
      taskIds: [changed.id],
      taskById: { [changed.id]: changed },
    });

    renderList();
    expect(screen.getByText("recoveryCenter.playbook.title")).toBeInTheDocument();
    const runButtons = screen.getAllByRole("button", { name: "recoveryCenter.playbook.run" });
    expect(runButtons).toHaveLength(2);
    // The restart entry documents the destructive consequence.
    expect(screen.getByText("recoveryCenter.playbook.restart.deletes")).toBeInTheDocument();
  });
});
