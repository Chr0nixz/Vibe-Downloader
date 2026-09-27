import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";
import { ResolveAttentionDialog } from "./ResolveAttentionDialog";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({ t: (key: string) => key }),
  };
});

function makeTask(): Task {
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
    totalSize: 1_000,
    downloadedBytes: 250,
    status: "downloading",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: false,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 1,
    speedBps: 100,
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
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

describe("ResolveAttentionDialog pause confirmation", () => {
  it("explains the byte cost and dispatches only after confirmation", () => {
    const onResolve = vi.fn();

    render(
      <ResolveAttentionDialog
        request={{ task: makeTask(), action: "pause" }}
        open
        onOpenChange={vi.fn()}
        onResolve={onResolve}
      />,
    );

    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(screen.getByText("recovery.pauseCost")).toBeInTheDocument();
    expect(onResolve).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "recoveryDialog.confirmPause" }));
    expect(onResolve).toHaveBeenCalledOnce();
  });
});
