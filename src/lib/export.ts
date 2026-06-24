import type { Task } from "@/types/task";
import { isTauriRuntime } from "@/lib/runtime";
import { createLogger } from "@/lib/logger";

const log = createLogger("export");

export type ExportFormat = "json" | "csv";

function taskToExportRow(task: Task): Record<string, string> {
  return {
    id: task.id,
    url: task.url,
    fileName: task.fileName,
    protocol: task.protocol,
    status: task.status,
    priority: task.priority,
    totalSize: String(task.totalSize),
    downloadedBytes: String(task.downloadedBytes),
    saveDir: task.saveDir,
    finalPath: task.finalPath ?? "",
    contentType: task.contentType ?? "",
    createdAt: task.createdAt,
    updatedAt: task.updatedAt,
    errorMessage: task.errorMessage ?? "",
    expectedHashSha256: task.expectedHashSha256 ?? "",
    hashStatus: task.hashStatus,
  };
}

function escapeCsvField(value: string): string {
  if (value.includes(",") || value.includes('"') || value.includes("\n")) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

function tasksToCsv(tasks: Task[]): string {
  const rows = tasks.map(taskToExportRow);
  if (rows.length === 0) return "";
  const headers = Object.keys(rows[0]);
  const headerLine = headers.join(",");
  const dataLines = rows.map((row) => headers.map((h) => escapeCsvField(row[h])).join(","));
  return [headerLine, ...dataLines].join("\n");
}

function tasksToJson(tasks: Task[]): string {
  const rows = tasks.map(taskToExportRow);
  return JSON.stringify(rows, null, 2);
}

function getMimeType(format: ExportFormat): string {
  return format === "json" ? "application/json" : "text/csv";
}

export function serializeTasks(tasks: Task[], format: ExportFormat): string {
  return format === "json" ? tasksToJson(tasks) : tasksToCsv(tasks);
}

export async function exportTasks(tasks: Task[], format: ExportFormat): Promise<boolean> {
  const content = serializeTasks(tasks, format);
  const extension = format;
  const mimeType = getMimeType(format);

  if (!isTauriRuntime()) {
    // Browser fallback: trigger download via blob
    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `vibe-tasks.${extension}`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    // Defer revocation so the download has time to start in all browsers.
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return true;
  }

  try {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: `vibe-tasks.${extension}`,
      filters: [
        { name: format === "json" ? "JSON" : "CSV", extensions: [extension] },
      ],
    });
    if (!path) return false;

    const { writeTextFile } = await import("@tauri-apps/plugin-fs");
    await writeTextFile(path, content);
    log.info("exported", { count: tasks.length, format, path });
    return true;
  } catch (err) {
    log.warn("export failed", err);
    return false;
  }
}
