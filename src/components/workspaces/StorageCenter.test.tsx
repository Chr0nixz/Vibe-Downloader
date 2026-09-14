import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { StorageScanResult } from "@/generated/bindings";
import { StorageCenter } from "./StorageCenter";

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

const scanStorage = vi.fn();
const cleanStorageArtifacts = vi.fn();
const cleanupTaskTempFiles = vi.fn();
const getLastStorageSweep = vi.fn();

vi.mock("@/lib/tauri", () => ({
  scanStorage: (...args: unknown[]) => scanStorage(...args),
  cleanStorageArtifacts: (...args: unknown[]) => cleanStorageArtifacts(...args),
  cleanupTaskTempFiles: (...args: unknown[]) => cleanupTaskTempFiles(...args),
  getLastStorageSweep: (...args: unknown[]) => getLastStorageSweep(...args),
  onStorageCleanupProgress: vi.fn(async () => () => {}),
}));

vi.mock("@/lib/format-date", () => ({
  formatDateTime: () => "2026-09-13 12:00",
}));

function makeScan(): StorageScanResult {
  return {
    scanId: "scan-1",
    scannedAt: "2026-09-13T12:00:00Z",
    dirs: [
      {
        path: "/downloads",
        totalBytes: "1000000",
        availableBytes: "500000",
        reclaimableBytes: "4000",
        resumableBytes: "2000",
        estimatedCompletableTasks: "10",
        truncated: false,
      },
    ],
    items: [
      {
        id: "/downloads/orphan.0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b.vibe-downloading",
        kind: "temp_file",
        saveDir: "/downloads",
        fileName: "orphan.bin.vibe-downloading",
        bytes: "3000",
        modifiedAt: null,
        reclaimable: true,
        reason: "no_owner",
        ownerTaskId: null,
        taskFileName: null,
        ownerProtocol: null,
      },
      {
        id: "/downloads/done.vibe-downloading",
        kind: "legacy_temp_file",
        saveDir: "/downloads",
        fileName: "done.bin.vibe-downloading",
        bytes: "1000",
        modifiedAt: null,
        reclaimable: true,
        reason: "owner_completed",
        ownerTaskId: null,
        taskFileName: null,
        ownerProtocol: null,
      },
      {
        id: "/downloads/kept.vibe-downloading",
        kind: "temp_file",
        saveDir: "/downloads",
        fileName: "paused.bin.vibe-downloading",
        bytes: "2000",
        modifiedAt: null,
        reclaimable: false,
        reason: "owner_resumable",
        ownerTaskId: "task-1",
        taskFileName: "paused.bin",
        ownerProtocol: "https",
      },
    ],
  };
}

describe("StorageCenter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    scanStorage.mockResolvedValue(makeScan());
    getLastStorageSweep.mockResolvedValue(null);
    cleanStorageArtifacts.mockResolvedValue({
      requestId: "r1",
      mode: "all_reclaimable",
      removedCount: 2,
      skippedCount: 0,
      failedCount: 0,
      reclaimedBytes: "4000",
      outcomes: [],
      resumeDiscarded: false,
    });
  });

  it("renders the directory overview and artifact groups after scan", async () => {
    render(<StorageCenter />);
    await waitFor(() => expect(scanStorage).toHaveBeenCalled());
    expect(await screen.findByText("storageCenter.overviewTitle")).toBeInTheDocument();
    expect(await screen.findByText("orphan.bin.vibe-downloading")).toBeInTheDocument();
    expect(screen.getByText("storageCenter.reason.no_owner")).toBeInTheDocument();
    // Resumable artifacts show up in their own read-only section.
    expect(screen.getByText("paused.bin")).toBeInTheDocument();
    expect(screen.getByText("storageCenter.resumableHint")).toBeInTheDocument();
  });

  it("shows an error banner with retry when the scan fails", async () => {
    scanStorage.mockRejectedValue(new Error("boom"));
    render(<StorageCenter />);
    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
    expect(screen.getByText("storageCenter.retryScan")).toBeInTheDocument();
  });

  it("runs the aggregate cleanup after confirmation", async () => {
    render(<StorageCenter />);
    await screen.findByText("orphan.bin.vibe-downloading");
    fireEvent.click(screen.getByRole("button", { name: "storageCenter.mode.all" }));
    // Hard confirm shows the permanent-deletion warning.
    expect(await screen.findByText("storageCenter.confirm.permanentWarning")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "storageCenter.confirm.confirm" }));
    await waitFor(() => expect(cleanStorageArtifacts).toHaveBeenCalledWith("all_reclaimable", undefined));
    // The page rescans after cleanup.
    await waitFor(() => expect(scanStorage.mock.calls.length).toBeGreaterThanOrEqual(2));
  });

  it("passes the selected item ids to the selected-mode cleanup", async () => {
    render(<StorageCenter />);
    await screen.findByText("orphan.bin.vibe-downloading");
    // Two reclaimable checkboxes exist (temp_file + legacy_temp_file); the
    // first group is the largest, which is the orphan temp.
    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes.length).toBeGreaterThanOrEqual(1);
    fireEvent.click(checkboxes[0]);
    fireEvent.click(screen.getByRole("button", { name: /storageCenter.cleanSelected/ }));
    expect(await screen.findByText("storageCenter.confirm.permanentWarning")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "storageCenter.confirm.confirm" }));
    await waitFor(() =>
      expect(cleanStorageArtifacts).toHaveBeenCalledWith("selected", [
        "/downloads/orphan.0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b.vibe-downloading",
      ]),
    );
  });

  it("keeps resumable artifacts only behind the abandon-resume confirm", async () => {
    render(<StorageCenter />);
    await screen.findByText("paused.bin");
    fireEvent.click(screen.getByRole("button", { name: /storageCenter.abandonResume/ }));
    expect(await screen.findByText(/storageCenter\.confirm\.taskDescription/)).toBeInTheDocument();
    cleanupTaskTempFiles.mockResolvedValue({
      requestId: "r2",
      mode: "selected",
      removedCount: 1,
      skippedCount: 0,
      failedCount: 0,
      reclaimedBytes: "0",
      outcomes: [],
      resumeDiscarded: true,
    });
    fireEvent.click(screen.getByRole("button", { name: "storageCenter.confirm.confirm" }));
    await waitFor(() => expect(cleanupTaskTempFiles).toHaveBeenCalledWith("task-1"));
  });
});
