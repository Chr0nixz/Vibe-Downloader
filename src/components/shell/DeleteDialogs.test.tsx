import { fireEvent, render, screen } from "@testing-library/react";
import { axe } from "jest-axe";
import { describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";
import { BulkDeleteDialog } from "./BulkDeleteDialog";
import { DeleteTaskDialog } from "./DeleteTaskDialog";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, values?: Record<string, string | number>) =>
        values ? `${key} ${Object.values(values).join(" ")}` : key,
    }),
  };
});

function makeTask(id: string, fileName = `超长文件名-${"数据".repeat(30)}-📦.zip`): Task {
  const now = "2026-01-01T00:00:00.000Z";
  return {
    id,
    url: `https://example.com/${id}.zip`,
    finalUrl: null,
    protocol: "https",
    taskKind: "single_file",
    fileName,
    saveDir: "D:\\Downloads",
    tempPath: null,
    finalPath: null,
    totalSize: 100,
    downloadedBytes: 0,
    status: "paused",
    etag: null,
    lastModified: null,
    contentType: "application/zip",
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
    files: [],
    createdAt: now,
    updatedAt: now,
  };
}

describe("delete confirmation dialogs", () => {
  it("keeps a long filename accessible and supports cancel and confirm", async () => {
    const task = makeTask("task-1");
    const onOpenChange = vi.fn();
    const onDelete = vi.fn();
    render(<DeleteTaskDialog task={task} open onOpenChange={onOpenChange} onDelete={onDelete} />);

    expect(screen.getByText(task.fileName, { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "deleteDialog.cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    fireEvent.click(screen.getByRole("button", { name: "deleteDialog.filesConfirm" }));
    expect(onDelete).toHaveBeenCalledTimes(1);

    const results = await axe(document.body);
    expect(results.violations).toEqual([]);
  });

  it("limits bulk names, reports the remainder, and remains accessible", async () => {
    const tasks = Array.from({ length: 8 }, (_, index) => makeTask(`task-${index}`, `archive-${index}.zip`));
    const onDelete = vi.fn();
    render(<BulkDeleteDialog tasks={tasks} open onOpenChange={vi.fn()} onDelete={onDelete} />);

    expect(screen.getByText("archive-0.zip")).toBeInTheDocument();
    expect(screen.queryByText("archive-7.zip")).not.toBeInTheDocument();
    expect(screen.getByText("deleteDialog.bulkMore 2")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "deleteDialog.bulkFilesConfirm 8" }));
    expect(onDelete).toHaveBeenCalledTimes(1);

    const results = await axe(document.body);
    expect(results.violations).toEqual([]);
  });
});
