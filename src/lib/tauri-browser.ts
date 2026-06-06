import type { CreateTaskInput, ProbeTaskInput, ProbeTaskPayload } from "@/generated/bindings";
import type { Task, TaskStatus } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";
import type { TaskProgressPayload } from "@/types/task-progress";

const STORAGE_KEY = "vibe-browser-mock-tasks";

type BrowserListener = (payload: TaskProgressPayload) => void;

let tasks: Task[] = loadStoredTasks() ?? buildBrowserMockTasks();
let progressTimer: ReturnType<typeof setInterval> | undefined;
const progressListeners = new Set<BrowserListener>();
const queueListeners = new Set<() => void>();

function nowIso(): string {
  return new Date().toISOString();
}

function persistTasks(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tasks));
  } catch {
    /* ignore quota / private mode */
  }
}

function loadStoredTasks(): Task[] | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Task[];
    return Array.isArray(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function emitQueueChanged(): void {
  for (const handler of queueListeners) handler();
}

function emitProgress(payload: TaskProgressPayload): void {
  for (const handler of progressListeners) handler(payload);
}

function browserTask(
  id: string,
  fileName: string,
  url: string,
  host: string,
  status: TaskStatus,
  totalSize: number,
  downloadedBytes: number,
  connectionCount: number,
  speedBps: number,
  healthSummary: string | null,
  timestamp: string,
): Task {
  const needsError = status === "failed" || status === "needs_attention";
  return normalizeTask({
    id,
    url,
    finalUrl: url,
    fileName,
    saveDir: "~/Downloads",
    tempPath: null,
    finalPath: status === "completed" ? `~/Downloads/${fileName}` : null,
    totalSize: String(totalSize),
    downloadedBytes: String(downloadedBytes),
    status,
    etag: null,
    lastModified: null,
    contentType: null,
    supportsRange: true,
    sourceHost: host,
    connectionCount,
    speedBps: String(speedBps),
    healthSummary,
    errorMessage: needsError ? healthSummary : null,
    createdAt: timestamp,
    updatedAt: timestamp,
  });
}

export function buildBrowserMockTasks(): Task[] {
  const now = nowIso();
  return [
    browserTask("mock-ubuntu", "ubuntu-24.04.iso", "https://releases.ubuntu.com/noble/ubuntu-24.04-desktop-amd64.iso", "releases.ubuntu.com", "downloading", 4_200_000_000, 1_680_000_000, 8, 48_500_000, "Downloading steadily", now),
    browserTask("mock-node", "node-v22.pkg", "https://nodejs.org/dist/v22.0.0/node-v22.0.0.pkg", "nodejs.org", "downloading", 80_000_000, 52_000_000, 4, 12_400_000, "Server limit detected", now),
    browserTask("mock-rust", "rust-docs.pdf", "https://doc.rust-lang.org/book.pdf", "doc.rust-lang.org", "paused", 12_000_000, 4_800_000, 0, 0, null, now),
    browserTask("mock-game", "game-patch.zip", "https://cdn.example.com/patches/season-12.zip", "cdn.example.com", "queued", 2_400_000_000, 0, 0, 0, null, now),
    browserTask("mock-dataset", "dataset.tar.gz", "https://data.example.org/ml/dataset.tar.gz", "data.example.org", "retrying", 900_000_000, 120_000_000, 2, 3_200_000, "Network fluctuation, retrying", now),
    browserTask("mock-driver", "driver-setup.exe", "https://vendor.example.net/drivers/setup.exe", "vendor.example.net", "failed", 350_000_000, 89_000_000, 0, 0, "Resume unavailable", now),
    browserTask("mock-llm", "llm-weights.safetensors", "https://models.example.ai/weights/v3.safetensors", "models.example.ai", "needs_attention", 8_000_000_000, 2_100_000_000, 0, 0, "Remote file changed. Restart download to avoid corruption.", now),
    browserTask("mock-arch", "archlinux.iso", "https://mirror.archlinux.org/iso/latest/archlinux-x86_64.iso", "mirror.archlinux.org", "completed", 1_300_000_000, 1_300_000_000, 0, 0, "Completed", now),
    browserTask("mock-fonts", "fonts-bundle.zip", "https://github.com/google/fonts/archive/refs/heads/main.zip", "github.com", "waiting_network", 220_000_000, 45_000_000, 0, 0, "Waiting for network", now),
    browserTask("mock-vscode", "vscode.deb", "https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64", "code.visualstudio.com", "downloading", 95_000_000, 71_000_000, 2, 8_900_000, "Disk write slower than network", now),
  ];
}

function cloneTasks(): Task[] {
  return tasks.map((task) => ({ ...task }));
}

function updateTask(id: string, patch: Partial<Task>): Task {
  const index = tasks.findIndex((task) => task.id === id);
  if (index < 0) throw new Error(`Task not found: ${id}`);
  const next = { ...tasks[index], ...patch, updatedAt: nowIso() };
  tasks = [...tasks.slice(0, index), next, ...tasks.slice(index + 1)];
  persistTasks();
  emitQueueChanged();
  return next;
}

function tickActiveDownloads(): void {
  let changed = false;
  tasks = tasks.map((task) => {
    if (task.status !== "downloading" && task.status !== "retrying") return task;
    if (task.downloadedBytes >= task.totalSize) return task;
    const step = Math.max(64_000, Math.floor(task.speedBps / 4));
    const downloadedBytes = Math.min(task.totalSize, task.downloadedBytes + step);
    changed = true;
    const next = { ...task, downloadedBytes, updatedAt: nowIso() };
    emitProgress({
      taskId: next.id,
      downloadedBytes: String(next.downloadedBytes),
      totalSize: String(next.totalSize),
      speedBps: String(next.speedBps),
      status: next.status,
      connectionCount: next.connectionCount,
    });
    return next;
  });
  if (changed) {
    persistTasks();
    emitQueueChanged();
  }
}

function ensureProgressTimer(): void {
  if (progressTimer) return;
  progressTimer = setInterval(tickActiveDownloads, 1000);
}

function stopProgressTimerIfIdle(): void {
  const hasActive = tasks.some(
    (task) =>
      (task.status === "downloading" || task.status === "retrying") &&
      task.downloadedBytes < task.totalSize,
  );
  if (!hasActive && progressTimer) {
    clearInterval(progressTimer);
    progressTimer = undefined;
  }
}

export async function listTasks(): Promise<Task[]> {
  if (tasks.length === 0) {
    tasks = buildBrowserMockTasks();
    persistTasks();
  }
  ensureProgressTimer();
  return cloneTasks();
}

export async function getTask(id: string): Promise<Task | null> {
  return tasks.find((task) => task.id === id) ?? null;
}

export async function listTaskSegments(taskId: string): Promise<TaskSegment[]> {
  const task = tasks.find((entry) => entry.id === taskId);
  if (!task) return [];
  const total = Math.max(1, task.totalSize);
  const downloaded = Math.min(task.downloadedBytes, total);
  return [
    normalizeTaskSegment({
      id: `${taskId}-segment-0`,
      taskId,
      rangeStart: "0",
      rangeEnd: String(total - 1),
      downloadedUntil: String(downloaded),
      status:
        task.status === "completed"
          ? "completed"
          : task.status === "failed"
            ? "failed"
            : downloaded > 0
              ? "downloading"
              : "pending",
      retryCount: task.status === "retrying" ? 1 : 0,
      lastError: task.errorMessage,
    }),
  ];
}

export async function seedMockTasks(): Promise<Task[]> {
  tasks = buildBrowserMockTasks();
  persistTasks();
  emitQueueChanged();
  ensureProgressTimer();
  return cloneTasks();
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  const now = nowIso();
  const fileName = input.fileName?.trim() || "download.bin";
  const task = browserTask(
    `mock-${crypto.randomUUID()}`,
    fileName,
    input.url,
    new URL(input.url).host,
    "queued",
    0,
    0,
    0,
    0,
    null,
    now,
  );
  tasks = [task, ...tasks];
  persistTasks();
  emitQueueChanged();
  return task;
}

export async function probeTask(input: ProbeTaskInput): Promise<ProbeTaskPayload> {
  let host = "example.com";
  try {
    host = new URL(input.url).host;
  } catch {
    /* keep fallback host */
  }
  return {
    finalUrl: input.url,
    fileName: "download.bin",
    totalSize: "1048576",
    supportsRange: true,
    sourceHost: host,
    contentType: "application/octet-stream",
  };
}

export async function pauseTask(id: string): Promise<Task> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  return updateTask(id, { status: "paused", speedBps: 0, connectionCount: 0 });
}

export async function resumeTask(id: string): Promise<Task> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  const next = updateTask(id, {
    status: "downloading",
    speedBps: task.speedBps > 0 ? task.speedBps : 4_000_000,
    connectionCount: task.connectionCount > 0 ? task.connectionCount : 2,
  });
  ensureProgressTimer();
  return next;
}

export async function retryTask(id: string): Promise<Task> {
  return resumeTask(id);
}

export async function cancelTask(id: string): Promise<Task> {
  return pauseTask(id);
}

export async function deleteTask(id: string, _deleteFile = false): Promise<void> {
  tasks = tasks.filter((task) => task.id !== id);
  persistTasks();
  emitQueueChanged();
  stopProgressTimerIfIdle();
}

export async function openTaskFile(_id: string): Promise<void> {
  /* no-op in browser preview */
}

export async function openTaskFolder(_id: string): Promise<void> {
  /* no-op in browser preview */
}

export function onTaskProgress(handler: (payload: TaskProgressPayload) => void): Promise<() => void> {
  progressListeners.add(handler);
  ensureProgressTimer();
  return Promise.resolve(() => {
    progressListeners.delete(handler);
    stopProgressTimerIfIdle();
  });
}

export function onQueueChanged(handler: () => void): Promise<() => void> {
  queueListeners.add(handler);
  return Promise.resolve(() => {
    queueListeners.delete(handler);
  });
}
