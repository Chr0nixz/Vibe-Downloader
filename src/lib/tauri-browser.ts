import type {
  AppSettings,
  BatchImportResult,
  BrowserCaptureSettings,
  BrowserCaptureSettingsInput,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  BrowserIntegrationUpdateInput,
  ClipboardLinkDetectedPayload,
  CreateTaskInput,
  CursorPageInput,
  HashVerificationState,
  ImportUrlsInput,
  ListTasksCursorInput,
  ListTasksInput,
  ListTasksCursorResult,
  ProbeTaskInput,
  ProbeTaskPayload,
  RequestDiagnostic,
  ResolveTaskAttentionInput,
  SegmentSummary,
  TaskEvent,
  TorrentRuntimeSnapshot,
  UpdateSettingsInput,
} from "@/generated/bindings";
import type { Task, TaskStatus } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";
import type { TaskProgressPayload } from "@/types/task-progress";
import { createLogger } from "@/lib/logger";
import { sanitizeUrlForDisplay } from "@/lib/utils";

const log = createLogger("browser-mock");
const STORAGE_KEY = "vibe-browser-mock-tasks";
const SETTINGS_STORAGE_KEY = "vibe-browser-settings";
const BROWSER_CAPTURE_SETTINGS_KEY = "vibe-browser-capture-settings";
const browserExperimentalCaptureEnabled = ["1", "true", "yes", "on"].includes(
  String(import.meta.env.VITE_BROWSER_EXPERIMENTAL_CAPTURE ?? "").toLowerCase(),
);

type BrowserListener = (payload: TaskProgressPayload) => void;
type TaskUpdatedListener = (task: Task) => void;

let tasks: Task[] = loadStoredTasks() ?? buildBrowserMockTasks();
let taskEvents: TaskEvent[] = buildBrowserMockEvents(tasks);
let taskRequests: RequestDiagnostic[] = [];
let settings: AppSettings = loadStoredSettings() ?? {
  maxActiveTasks: 2,
  defaultSaveDir: "~/Downloads",
  globalSpeedLimitBps: null,
  multiConnectionThresholdBytes: String(16 * 1024 * 1024),
  segmentCount: 4,
  maxConnectionsPerHost: 8,
  systemNotifications: true,
  closeToTray: false,
  startOnBoot: false,
  floatingWindowEnabled: false,
  clipboardMonitorEnabled: true,
  fontFamily: "source_han_sans_sc",
  accentColor: "blue",
  proxyMode: "off",
  proxyUrl: "",
  proxyNoProxy: "",
  proxyUsername: "",
  proxyPasswordSaved: false,
};
let progressTimer: ReturnType<typeof setInterval> | undefined;
const progressListeners = new Set<BrowserListener>();
const taskUpdatedListeners = new Set<TaskUpdatedListener>();
const queueListeners = new Set<() => void>();
const settingsListeners = new Set<() => void>();
const browserIntegrationListeners = new Set<() => void>();
const browserIntegrationInstalled = new Set(["chrome", "edge"]);
let browserCaptureSettings: BrowserCaptureSettings =
  loadStoredBrowserCaptureSettings() ?? defaultBrowserCaptureSettings();

function nowIso(): string {
  return new Date().toISOString();
}

function persistTasks(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(tasks));
  } catch (error) {
    log.warn("failed to persist mock tasks", error);
  }
}

function persistSettings(): void {
  try {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  } catch (error) {
    log.warn("failed to persist mock settings", error);
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

function loadStoredSettings(): AppSettings | null {
  try {
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as AppSettings;
    if (
      typeof parsed.defaultSaveDir === "string" &&
      typeof parsed.maxActiveTasks === "number"
    ) {
      return {
        ...parsed,
        globalSpeedLimitBps:
          typeof parsed.globalSpeedLimitBps === "string"
            ? parsed.globalSpeedLimitBps
            : null,
        multiConnectionThresholdBytes:
          typeof parsed.multiConnectionThresholdBytes === "string"
            ? parsed.multiConnectionThresholdBytes
            : String(16 * 1024 * 1024),
        segmentCount:
          typeof parsed.segmentCount === "number"
            ? Math.min(8, Math.max(1, parsed.segmentCount))
            : 4,
        maxConnectionsPerHost:
          typeof parsed.maxConnectionsPerHost === "number"
            ? Math.min(16, Math.max(1, parsed.maxConnectionsPerHost))
            : 8,
        systemNotifications:
          typeof parsed.systemNotifications === "boolean"
            ? parsed.systemNotifications
            : true,
        closeToTray:
          typeof parsed.closeToTray === "boolean" ? parsed.closeToTray : false,
        startOnBoot:
          typeof parsed.startOnBoot === "boolean" ? parsed.startOnBoot : false,
        floatingWindowEnabled:
          typeof parsed.floatingWindowEnabled === "boolean"
            ? parsed.floatingWindowEnabled
            : false,
        clipboardMonitorEnabled:
          typeof parsed.clipboardMonitorEnabled === "boolean"
            ? parsed.clipboardMonitorEnabled
            : true,
        fontFamily:
          parsed.fontFamily === "system" || parsed.fontFamily === "source_han_sans_sc"
            ? parsed.fontFamily
            : "source_han_sans_sc",
        accentColor:
          parsed.accentColor === "blue" ||
          parsed.accentColor === "purple" ||
          parsed.accentColor === "teal" ||
          parsed.accentColor === "green" ||
          parsed.accentColor === "orange" ||
          parsed.accentColor === "rose" ||
          parsed.accentColor === "indigo" ||
          parsed.accentColor === "amber"
            ? parsed.accentColor
            : "blue",
        proxyMode:
          parsed.proxyMode === "system" || parsed.proxyMode === "custom" ? parsed.proxyMode : "off",
        proxyUrl: typeof parsed.proxyUrl === "string" ? parsed.proxyUrl : "",
        proxyNoProxy: typeof parsed.proxyNoProxy === "string" ? parsed.proxyNoProxy : "",
        proxyUsername: typeof parsed.proxyUsername === "string" ? parsed.proxyUsername : "",
        proxyPasswordSaved: Boolean(parsed.proxyPasswordSaved),
      };
    }
    return null;
  } catch {
    return null;
  }
}

function defaultBrowserCaptureSettings(): BrowserCaptureSettings {
  return {
    autoIntercept: browserExperimentalCaptureEnabled,
    forwardHeaders: false,
    forwardHeadersMode: browserExperimentalCaptureEnabled ? "ask" : "disabled",
    minSizeBytes: "0",
    fileExtensions: ["zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "pdf"],
    siteRules: [],
  };
}

function loadStoredBrowserCaptureSettings(): BrowserCaptureSettings | null {
  try {
    const raw = localStorage.getItem(BROWSER_CAPTURE_SETTINGS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return clampBrowserCaptureSettings({
      ...defaultBrowserCaptureSettings(),
      ...parsed,
      forwardHeadersMode:
        parsed.forwardHeadersMode ??
        (typeof parsed.forwardHeaders === "boolean"
          ? parsed.forwardHeaders
            ? "enabled"
            : "disabled"
          : "ask"),
      forwardHeaders:
        parsed.forwardHeadersMode === "enabled" ||
        (parsed.forwardHeadersMode == null && parsed.forwardHeaders === true),
      fileExtensions: Array.isArray(parsed.fileExtensions)
        ? parsed.fileExtensions
        : defaultBrowserCaptureSettings().fileExtensions,
      siteRules: Array.isArray(parsed.siteRules) ? parsed.siteRules : [],
    });
  } catch {
    return null;
  }
}

function clampBrowserCaptureSettings(settings: BrowserCaptureSettings): BrowserCaptureSettings {
  if (browserExperimentalCaptureEnabled) return settings;
  return {
    ...settings,
    autoIntercept: false,
    forwardHeaders: false,
    forwardHeadersMode: "disabled",
  };
}

function persistBrowserCaptureSettings() {
  localStorage.setItem(BROWSER_CAPTURE_SETTINGS_KEY, JSON.stringify(browserCaptureSettings));
}

function emitQueueChanged(): void {
  for (const handler of queueListeners) handler();
}

function emitSettingsChanged(): void {
  for (const handler of settingsListeners) handler();
}

function emitBrowserIntegrationChanged(): void {
  for (const handler of browserIntegrationListeners) handler();
}

function emitProgress(payload: TaskProgressPayload): void {
  for (const handler of progressListeners) handler(payload);
}

function emitTaskUpdated(task: Task): void {
  for (const handler of taskUpdatedListeners) handler({ ...task });
}

function logTaskEvent(taskId: string, eventType: string, payload: string | null = null): void {
  taskEvents = [
    {
      id: String(Date.now() + taskEvents.length),
      taskId,
      eventType,
      payload,
      createdAt: nowIso(),
    },
    ...taskEvents,
  ].slice(0, 500);
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
  const parsed = safeUrl(url);
  const protocol = parsed?.protocol.replace(":", "") || "https";
  return normalizeTask({
    id,
    url,
    finalUrl: url,
    protocol,
    taskKind: "single_file",
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
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: host,
    connectionCount,
    speedBps: String(speedBps),
    healthSummary,
    errorMessage: needsError ? healthSummary : null,
    errorCode: null,
    recoveryActions: [],
    retryAfterAt: null,
    failureCategory: needsError ? "other" : null,
    expectedHashSha256: null,
    actualHashSha256: null,
    hashStatus: "not_requested",
    hashError: null,
    hashVerifiedAt: null,
    files: [
      {
        id: `${id}-file-0`,
        taskId: id,
        relativePath: fileName,
        fileName,
        saveDir: "~/Downloads",
        tempPath: null,
        finalPath: status === "completed" ? `~/Downloads/${fileName}` : null,
        totalSize: String(totalSize),
        downloadedBytes: String(downloadedBytes),
        selected: true,
        status,
        contentType: null,
      },
    ],
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

function buildBrowserMockEvents(nextTasks: Task[]): TaskEvent[] {
  return nextTasks.flatMap((task, index) => {
    const baseId = index * 10;
    const events: TaskEvent[] = [
      {
        id: String(baseId + 1),
        taskId: task.id,
        eventType: "created",
        payload: null,
        createdAt: task.createdAt,
      },
    ];
    if (task.status === "completed") {
      events.unshift({
        id: String(baseId + 2),
        taskId: task.id,
        eventType: "completed",
        payload: null,
        createdAt: task.updatedAt,
      });
    } else if (task.status === "failed" || task.status === "needs_attention") {
      events.unshift({
        id: String(baseId + 2),
        taskId: task.id,
        eventType: "failed",
        payload: task.errorMessage,
        createdAt: task.updatedAt,
      });
    }
    return events;
  });
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
  emitTaskUpdated(next);
  return next;
}

function scheduleBrowserQueue(): void {
  const activeCount = tasks.filter(
    (task) => task.status === "downloading" || task.status === "retrying",
  ).length;
  let available = Math.max(0, settings.maxActiveTasks - activeCount);
  if (available === 0) return;

  let changed = false;
  tasks = tasks.map((task) => {
    if (available <= 0 || task.status !== "queued") return task;
    available -= 1;
    changed = true;
    const next: Task = {
      ...task,
      status: "downloading",
      speedBps: task.speedBps > 0 ? task.speedBps : 4_000_000,
      connectionCount: Math.min(
        settings.maxConnectionsPerHost,
        task.connectionCount > 0 ? task.connectionCount : settings.segmentCount,
      ),
      healthSummary: "Downloading",
      updatedAt: nowIso(),
    };
    logTaskEvent(task.id, "started");
    emitTaskUpdated(next);
    return next;
  });

  if (changed) {
    persistTasks();
    emitQueueChanged();
    ensureProgressTimer();
  }
}

function tickActiveDownloads(): void {
  let changed = false;
  tasks = tasks.map((task) => {
    if (task.status !== "downloading" && task.status !== "retrying") return task;
    if (task.downloadedBytes >= task.totalSize) return task;
    const step = Math.max(64_000, Math.floor(task.speedBps / 4));
    const downloadedBytes = Math.min(task.totalSize, task.downloadedBytes + step);
    const completed = downloadedBytes >= task.totalSize;
    changed = true;
    const next = {
      ...task,
      downloadedBytes,
      status: completed ? ("completed" as const) : task.status,
      speedBps: completed ? 0 : task.speedBps,
      connectionCount: completed ? 0 : task.connectionCount,
      healthSummary: completed ? "Completed" : task.healthSummary,
      updatedAt: nowIso(),
    };
    if (completed) {
      logTaskEvent(task.id, "completed");
      emitTaskUpdated(next);
    }
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
    scheduleBrowserQueue();
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

async function mockTaskSegmentsForTask(taskId: string): Promise<TaskSegment[]> {
  const task = tasks.find((entry) => entry.id === taskId);
  if (!task) return [];
  const total = Math.max(1, task.totalSize);
  const downloaded = Math.min(task.downloadedBytes, total);
  return [
    normalizeTaskSegment({
      id: `${taskId}-segment-0`,
      taskId,
      fileId: task.files[0]?.id ?? null,
      unitKind: "http_range",
      rangeStart: "0",
      rangeEnd: String(total - 1),
      downloadedUntil: String(downloaded),
      speedBps: String(task.status === "downloading" ? task.speedBps : 0),
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

export async function listTasksPage(input: ListTasksInput) {
  const pageSize = Math.max(1, Math.min(500, input.pageSize ?? 100));
  const page = Math.max(0, input.page ?? 0);
  const all = await listTasks();
  const start = page * pageSize;
  return {
    items: all.slice(start, start + pageSize),
    total: all.length,
    page,
    pageSize,
  };
}

export async function listTasksCursor(input: ListTasksCursorInput) {
  const pageSize = Math.max(1, Math.min(500, input.pageSize ?? 100));
  const start = Math.max(0, Number(input.cursor ?? 0) || 0);
  const all = await listTasks();
  const items = all.slice(start, start + pageSize);
  const next = start + items.length;
  const failureCategories = Array.from(
    new Set(all.map((task) => task.failureCategory ?? mockFailureCategory(task)).filter(Boolean)),
  ).sort() as string[];
  return {
    items,
    nextCursor: next < all.length ? String(next) : null,
    totalEstimate: all.length,
    filterOptions: {
      sources: Array.from(new Set(all.map((task) => task.sourceKey).filter(Boolean))).sort(),
      failureCategories,
    },
  } satisfies {
    items: Task[];
    nextCursor: string | null;
    totalEstimate: number;
    filterOptions: ListTasksCursorResult["filterOptions"];
  };
}

export async function listSegments(taskId: string, _page = 0, _pageSize = 100): Promise<TaskSegment[]> {
  return mockTaskSegmentsForTask(taskId);
}

export async function listSegmentsPage(input: CursorPageInput) {
  const pageSize = Math.max(1, Math.min(500, input.pageSize ?? 100));
  const start = Math.max(0, Number(input.cursor ?? 0) || 0);
  const all = await mockTaskSegmentsForTask(input.taskId);
  const items = all.slice(start, start + pageSize);
  const next = start + items.length;
  return { items, nextCursor: next < all.length ? String(next) : null };
}

export async function getSegmentSummary(taskId: string): Promise<SegmentSummary> {
  const segments = await mockTaskSegmentsForTask(taskId);
  return {
    total: segments.length,
    active: segments.filter((segment) => segment.status === "downloading").length,
    completed: segments.filter((segment) => segment.status === "completed").length,
    failed: segments.filter((segment) => segment.status === "failed").length,
    downloadedBytes: String(
      segments.reduce(
        (sum, segment) =>
          sum + Math.max(0, segment.downloadedUntil - segment.rangeStart),
        0,
      ),
    ),
    speedBps: String(segments.reduce((sum, segment) => sum + segment.speedBps, 0)),
  };
}

export async function getTorrentRuntimeSnapshot(
  taskId: string,
): Promise<TorrentRuntimeSnapshot | null> {
  const task = tasks.find((entry) => entry.id === taskId);
  if (!task || (task.protocol !== "bt" && task.protocol !== "magnet")) return null;
  const live = task.status === "downloading" || task.status === "retrying";
  const uploadSpeedBps = live ? Math.max(64_000, Math.floor(task.speedBps / 12)) : 0;
  const uploaded = Math.floor(task.downloadedBytes / 20);
  return {
    taskId,
    metadataStatus: task.files.length > 0 ? "ready" : "fetching",
    completedPieces: String(Math.floor(task.downloadedBytes / (4 * 1024 * 1024))),
    verifiedPieces: String(Math.floor(task.downloadedBytes / (4 * 1024 * 1024))),
    peerCount: live ? "12" : "0",
    seedCount: live ? "4" : "0",
    uploadBytes: String(uploaded),
    uploadSpeedBps: String(uploadSpeedBps),
    ratio: task.downloadedBytes > 0 ? uploaded / task.downloadedBytes : 0,
    updatedAt: task.updatedAt,
  };
}

async function mockTaskEventsForTask(taskId: string): Promise<TaskEvent[]> {
  return taskEvents.filter((event) => event.taskId === taskId).slice(0, 100);
}

export async function listTaskEventsPage(input: CursorPageInput) {
  const pageSize = Math.max(1, Math.min(500, input.pageSize ?? 100));
  const start = Math.max(0, Number(input.cursor ?? 0) || 0);
  const all = await mockTaskEventsForTask(input.taskId);
  const items = all.slice(start, start + pageSize);
  const next = start + items.length;
  return { items, nextCursor: next < all.length ? String(next) : null };
}

async function mockTaskRequestsForTask(taskId: string): Promise<RequestDiagnostic[]> {
  const task = tasks.find((entry) => entry.id === taskId);
  if (!task) return [];
  if (!taskRequests.some((request) => request.taskId === taskId)) {
    taskRequests = [
      {
        id: `${taskId}-request-0`,
        taskId,
        method: "GET",
        url: sanitizeUrlForDisplay(task.url),
        rangeHeader: task.supportsParallel ? "bytes=0-" : null,
        statusCode: task.status === "failed" ? 503 : 206,
        etag: task.etag,
        lastModified: task.lastModified,
        contentLength: String(task.totalSize),
        errorMessage: task.errorMessage,
        retryCount: task.status === "retrying" ? 1 : 0,
        durationMs: "128",
        createdAt: task.updatedAt,
      },
      ...taskRequests,
    ];
  }
  return taskRequests.filter((request) => request.taskId === taskId).slice(0, 100);
}

export async function listTaskRequestsPage(input: CursorPageInput) {
  const pageSize = Math.max(1, Math.min(500, input.pageSize ?? 100));
  const start = Math.max(0, Number(input.cursor ?? 0) || 0);
  const all = await mockTaskRequestsForTask(input.taskId);
  const items = all.slice(start, start + pageSize);
  const next = start + items.length;
  return { items, nextCursor: next < all.length ? String(next) : null };
}

function mockFailureCategory(task: Task): string | null {
  if (!task.errorCode && !task.errorMessage) return null;
  if (task.errorCode?.startsWith("http_")) return "http";
  if (task.errorCode?.includes("disk")) return "disk_write";
  if (task.errorCode?.includes("temp_file")) return "temp_file";
  if (task.errorCode?.includes("resume")) return "resume_unavailable";
  if (task.errorCode?.includes("remote")) return "remote_changed";
  if (task.errorCode?.includes("auth_headers")) return "auth";
  return "other";
}

export async function seedMockTasks(): Promise<Task[]> {
  tasks = buildBrowserMockTasks();
  taskEvents = buildBrowserMockEvents(tasks);
  persistTasks();
  scheduleBrowserQueue();
  emitQueueChanged();
  ensureProgressTimer();
  return cloneTasks();
}

export async function getSettings(): Promise<AppSettings> {
  return { ...settings };
}

export async function updateSettings(input: UpdateSettingsInput): Promise<AppSettings> {
  const nextSaveDir =
    input.defaultSaveDir === null || input.defaultSaveDir === undefined
      ? settings.defaultSaveDir
      : input.defaultSaveDir.trim() || "~/Downloads";
  settings = {
    maxActiveTasks: Math.min(8, Math.max(1, input.maxActiveTasks ?? settings.maxActiveTasks)),
    defaultSaveDir: nextSaveDir,
    globalSpeedLimitBps:
      input.globalSpeedLimitBps === null || input.globalSpeedLimitBps === undefined
        ? null
        : normalizeSpeedLimit(input.globalSpeedLimitBps),
    multiConnectionThresholdBytes:
      input.multiConnectionThresholdBytes === null ||
      input.multiConnectionThresholdBytes === undefined
        ? settings.multiConnectionThresholdBytes
        : normalizeByteThreshold(input.multiConnectionThresholdBytes),
    segmentCount: Math.min(8, Math.max(1, input.segmentCount ?? settings.segmentCount)),
    maxConnectionsPerHost: Math.min(
      16,
      Math.max(1, input.maxConnectionsPerHost ?? settings.maxConnectionsPerHost),
    ),
    systemNotifications: input.systemNotifications ?? settings.systemNotifications,
    closeToTray: input.closeToTray ?? settings.closeToTray,
    startOnBoot: input.startOnBoot ?? settings.startOnBoot,
    floatingWindowEnabled:
      input.floatingWindowEnabled ?? settings.floatingWindowEnabled,
    clipboardMonitorEnabled:
      input.clipboardMonitorEnabled ?? settings.clipboardMonitorEnabled,
    fontFamily: input.fontFamily ?? settings.fontFamily,
    accentColor: input.accentColor ?? settings.accentColor,
    proxyMode: input.proxyMode ?? settings.proxyMode,
    proxyUrl: input.proxyUrl ?? settings.proxyUrl,
    proxyNoProxy: input.proxyNoProxy ?? settings.proxyNoProxy,
    proxyUsername: input.proxyUsername ?? settings.proxyUsername,
    proxyPasswordSaved: input.clearProxyPassword
      ? false
      : input.proxyPassword
        ? true
        : settings.proxyPasswordSaved,
  };
  persistSettings();
  emitSettingsChanged();
  scheduleBrowserQueue();
  return { ...settings };
}

function normalizeSpeedLimit(value: string): string | null {
  const parsed = Number(value.trim());
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return String(Math.floor(parsed));
}

function normalizeByteThreshold(value: string): string {
  const parsed = Number(value.trim());
  if (!Number.isFinite(parsed) || parsed < 0) return "0";
  return String(Math.floor(parsed));
}

export async function openDirectoryPicker(): Promise<string | null> {
  return "~/Downloads";
}

export interface PickedFile {
  path: string;
  name: string;
}

export async function openFilePicker(
  _filters?: { name: string; extensions: string[] }[],
): Promise<PickedFile | null> {
  return null;
}

export async function getBrowserIntegrationStatus(): Promise<BrowserIntegrationStatus> {
  return {
    nativeHostName: "com.vibe_downloader.native_host",
    nativeHostPath: "~/Applications/Vibe Downloader/vibe-native-host",
    extensionCorePath: "browser/extension-core",
    experimentalCaptureEnabled: browserExperimentalCaptureEnabled,
    realtime: {
      wsUrl: "ws://127.0.0.1:48365/browser/ws",
      connected: true,
    },
    capture: { ...browserCaptureSettings },
    browsers: [
      "chrome",
      "edge",
      "firefox",
      "safari",
      "brave",
      "opera",
      "vivaldi",
      "chromium",
    ].map((browser) => ({
      browser: browser as BrowserIntegrationStatus["browsers"][number]["browser"],
      displayName: browserDisplayName(browser),
      supportedOnPlatform: browser !== "safari",
      detected: browser !== "safari",
      manifestInstalled: browserIntegrationInstalled.has(browser),
      manifestPath: `~/Library/Application Support/${browser}/NativeMessagingHosts/com.vibe_downloader.native_host.json`,
      extensionLoadPath: "browser/dist/chromium",
      extensionId:
        browser === "firefox"
          ? "vibe-downloader@local"
          : browser === "safari"
            ? null
            : "abcdefghijklmnopabcdefghijklmnop",
      profile: "dev",
      lastError: null,
    })),
  };
}

export async function getBrowserCaptureSettings(): Promise<BrowserCaptureSettings> {
  return { ...browserCaptureSettings };
}

export async function updateBrowserCaptureSettings(
  input: BrowserCaptureSettingsInput,
): Promise<BrowserCaptureSettings> {
  const forwardHeadersMode =
    input.forwardHeadersMode ??
    (input.forwardHeaders == null
      ? browserCaptureSettings.forwardHeadersMode
      : input.forwardHeaders
        ? "enabled"
        : "disabled");
  browserCaptureSettings = clampBrowserCaptureSettings({
    ...browserCaptureSettings,
    autoIntercept: input.autoIntercept ?? browserCaptureSettings.autoIntercept,
    forwardHeadersMode,
    forwardHeaders: forwardHeadersMode === "enabled",
    minSizeBytes: input.minSizeBytes ?? browserCaptureSettings.minSizeBytes,
    fileExtensions: input.fileExtensions ?? browserCaptureSettings.fileExtensions,
    siteRules: input.siteRules ?? browserCaptureSettings.siteRules,
  });
  persistBrowserCaptureSettings();
  emitBrowserIntegrationChanged();
  return { ...browserCaptureSettings };
}

export async function installBrowserIntegration(
  input: BrowserIntegrationUpdateInput,
): Promise<BrowserIntegrationStatus> {
  input.browsers.forEach((browser) => browserIntegrationInstalled.add(browser));
  emitBrowserIntegrationChanged();
  return getBrowserIntegrationStatus();
}

export async function uninstallBrowserIntegration(
  input: BrowserIntegrationUpdateInput,
): Promise<BrowserIntegrationStatus> {
  input.browsers.forEach((browser) => browserIntegrationInstalled.delete(browser));
  emitBrowserIntegrationChanged();
  return getBrowserIntegrationStatus();
}

export async function exportBrowserExtensionPackages(): Promise<BrowserExtensionExportResult> {
  const outputDir = "~/Downloads/Vibe Downloader Extensions/v0.1.0";
  return {
    outputDir,
    installGuidePath: `${outputDir}/INSTALL.md`,
    packages: [
      ["Chrome, Brave, Vivaldi, Chromium", "vibe-downloader-chromium-v0.1.0.zip"],
      ["Microsoft Edge", "vibe-downloader-edge-v0.1.0.zip"],
      ["Mozilla Firefox", "vibe-downloader-firefox-v0.1.0.xpi"],
      ["Opera", "vibe-downloader-opera-v0.1.0.zip"],
    ].map(([target, fileName]) => ({
      target,
      packagePath: `${outputDir}/${fileName}`,
      sha256: "mock-sha256",
      installNote: "Mock browser preview package.",
    })),
  };
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  const now = nowIso();
  const normalizedUrl = input.url.trim();
  const probe =
    input.probeSnapshot?.inputUrl.trim() === normalizedUrl
      ? input.probeSnapshot
      : await probeTask({ url: input.url });
  const fileName = input.fileName?.trim() || probe.fileName || "download.bin";
  const isTorrentMultiFile =
    (probe.protocol === "bt" || probe.protocol === "magnet") && probe.files.length > 1;
  if (isTorrentMultiFile && input.selectedFilePaths && input.selectedFilePaths.length === 0) {
    throw new Error("Select at least one torrent file to download.");
  }
  const selectedFilePaths = new Set(
    isTorrentMultiFile ? (input.selectedFilePaths ?? []).filter(Boolean) : [],
  );
  if (selectedFilePaths.size > 0) {
    const availablePaths = new Set(probe.files.map((file) => file.relativePath));
    const unknown = Array.from(selectedFilePaths).filter((path) => !availablePaths.has(path));
    if (unknown.length > 0) {
      throw new Error(`Unknown torrent file selection: ${unknown[0]}`);
    }
  }
  const probeFiles =
    probe.files.length > 0
      ? probe.files
      : [{ relativePath: fileName, size: probe.totalSize, contentType: probe.contentType }];
  const taskFiles =
    selectedFilePaths.size > 0
      ? probeFiles.filter((file) => selectedFilePaths.has(file.relativePath))
      : probeFiles;
  const taskTotalSize =
    taskFiles.reduce((sum, file) => sum + parseNumericString(file.size), 0) ||
    parseNumericString(probe.totalSize);
  const parsed = safeUrl(input.url);
  const task = browserTask(
    `mock-${crypto.randomUUID()}`,
    fileName,
    input.url,
    probe.sourceKey || parsed?.host || "example.com",
    "queued",
    taskTotalSize,
    0,
    0,
    0,
    null,
    now,
  );
  const nextTask = {
    ...task,
    totalSize: taskTotalSize,
    protocol: probe.protocol,
    taskKind: probe.taskKind,
    supportsMultiFile: probe.capabilities.supportsMultiFile,
    sourceKey: probe.sourceKey,
    saveDir: input.saveDir?.trim() || settings.defaultSaveDir,
    files: taskFiles.map((file, index) => ({
      id: `${task.id}-file-${index}`,
      taskId: task.id,
      relativePath: file.relativePath,
      fileName: fileNameFromRelativePath(file.relativePath),
      saveDir: input.saveDir?.trim() || settings.defaultSaveDir,
      tempPath: null,
      finalPath: null,
      totalSize: parseNumericString(file.size),
      downloadedBytes: 0,
      selected: selectedFilePaths.size === 0 || selectedFilePaths.has(file.relativePath),
      status: "queued" as const,
      contentType: file.contentType,
    })),
  };
  tasks = [nextTask, ...tasks];
  logTaskEvent(task.id, "created");
  persistTasks();
  scheduleBrowserQueue();
  emitQueueChanged();
  emitTaskUpdated(nextTask);
  return nextTask;
}

export async function importUrls(input: ImportUrlsInput): Promise<BatchImportResult> {
  const urls = input.input
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const seen = new Set<string>();
  const items: BatchImportResult["items"] = [];
  for (const raw of urls) {
    let normalized: string | null = null;
    try {
      normalized = new URL(raw).toString();
    } catch {
      items.push({
        inputUrl: raw,
        normalizedUrl: null,
        duplicate: false,
        valid: false,
        fileName: null,
        totalSize: null,
        contentType: null,
        supportsResume: false,
        errorMessage: "URL is invalid.",
        task: null,
      });
      continue;
    }
    const duplicate = seen.has(normalized);
    seen.add(normalized);
    const probe = duplicate ? null : await probeTask({ url: normalized });
    const task =
      !duplicate && input.create
        ? await createTask({
            url: normalized,
            saveDir: input.saveDir,
            fileName: probe?.fileName ?? null,
            expectedHashSha256: null,
            probeSnapshot: probe,
            selectedFilePaths: null,
          })
        : null;
    items.push({
      inputUrl: raw,
      normalizedUrl: normalized,
      duplicate,
      valid: !duplicate,
      fileName: probe?.fileName ?? null,
      totalSize: probe?.totalSize ?? null,
      contentType: probe?.contentType ?? null,
      supportsResume: probe?.capabilities.supportsResume ?? false,
      errorMessage: duplicate ? "Duplicate URL in this import." : null,
      task: task as unknown as BatchImportResult["items"][number]["task"],
    });
  }
  return {
    items,
    createdCount: items.filter((item) => item.task).length,
    failedCount: items.filter((item) => !item.valid).length,
    duplicateCount: items.filter((item) => item.duplicate).length,
  };
}

export async function verifyTaskHash(id: string): Promise<HashVerificationState> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  return {
    taskId: id,
    expectedSha256: task.expectedHashSha256,
    actualSha256: task.actualHashSha256,
    status: task.hashStatus,
    errorMessage: task.hashError,
    verifiedAt: task.hashVerifiedAt,
  };
}

export async function probeTask(input: ProbeTaskInput): Promise<ProbeTaskPayload> {
  if (input.url.trim().startsWith("magnet:")) {
    const hash = input.url.match(/btih:([^&]+)/i)?.[1]?.toLowerCase() ?? "mock";
    const name = decodeURIComponent(input.url.match(/[?&]dn=([^&]+)/)?.[1] ?? `magnet-${hash}`);
    return {
      inputUrl: input.url,
      finalUrl: input.url,
      fileName: name,
      protocol: "bt",
      taskKind: "multi_file",
      capabilities: {
        supportsResume: true,
        supportsParallel: true,
        supportsMultiFile: true,
      },
      files: [],
      totalSize: "0",
      sourceKey: `bt:${hash}`,
      contentType: "application/x-magnet",
      etag: null,
      lastModified: null,
      probedAt: nowIso(),
    };
  }

  const parsed = safeUrl(input.url);
  const protocol = parsed?.protocol.replace(":", "") || "http";
  const host = parsed?.host ?? "example.com";
  return {
    inputUrl: input.url,
    finalUrl: input.url,
    fileName: "download.bin",
    protocol,
    taskKind: "single_file",
    capabilities: {
      supportsResume: true,
      supportsParallel: true,
      supportsMultiFile: false,
    },
    files: [
      {
        relativePath: "download.bin",
        size: "1048576",
        contentType: "application/octet-stream",
      },
    ],
    totalSize: "1048576",
    sourceKey: `${protocol}://${host}`,
    contentType: "application/octet-stream",
    etag: "\"mock-etag\"",
    lastModified: nowIso(),
    probedAt: nowIso(),
  };
}

function safeUrl(value: string): URL | null {
  try {
    return new URL(value);
  } catch {
    return null;
  }
}

function parseNumericString(value: string | null | undefined): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : 0;
}

function fileNameFromRelativePath(value: string): string {
  return value.replace(/[\\/]/g, "/").split("/").pop() || value || "download.bin";
}

export async function pauseTask(id: string): Promise<Task> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  const next = updateTask(id, { status: "paused", speedBps: 0, connectionCount: 0 });
  logTaskEvent(id, "paused");
  scheduleBrowserQueue();
  return next;
}

export async function resumeTask(id: string): Promise<Task> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  const next = updateTask(id, {
    status: "queued",
    speedBps: 0,
    connectionCount: 0,
    healthSummary: "Queued",
  });
  logTaskEvent(id, "resumed");
  scheduleBrowserQueue();
  return tasks.find((entry) => entry.id === next.id) ?? next;
}

export async function retryTask(id: string): Promise<Task> {
  logTaskEvent(id, "retrying");
  return resumeTask(id);
}

export async function resolveTaskAttention(input: ResolveTaskAttentionInput): Promise<Task> {
  const task = tasks.find((entry) => entry.id === input.id);
  if (!task) throw new Error(`Task not found: ${input.id}`);
  if (input.action === "open_folder" || input.action === "check_url") {
    return task;
  }
  if (input.action === "choose_another_name" || input.action === "choose_another_folder") {
    const fileName = input.fileName?.trim() || task.fileName;
    const saveDir = input.saveDir?.trim() || task.saveDir;
    updateTask(input.id, {
      fileName,
      saveDir,
      finalPath: `${saveDir}/${fileName}`,
    });
  }
  if (input.action === "restart") {
    updateTask(input.id, {
      downloadedBytes: 0,
      speedBps: 0,
      connectionCount: 0,
      healthSummary: "Queued",
      errorMessage: null,
    });
  }
  return resumeTask(input.id);
}

export async function cancelTask(id: string): Promise<Task> {
  return pauseTask(id);
}

export async function deleteTask(id: string, _deleteFile = false): Promise<void> {
  tasks = tasks.filter((task) => task.id !== id);
  persistTasks();
  emitQueueChanged();
  scheduleBrowserQueue();
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

export function onTaskUpdated(handler: (task: Task) => void): Promise<() => void> {
  taskUpdatedListeners.add(handler);
  return Promise.resolve(() => {
    taskUpdatedListeners.delete(handler);
  });
}

export function onQueueChanged(handler: () => void): Promise<() => void> {
  queueListeners.add(handler);
  return Promise.resolve(() => {
    queueListeners.delete(handler);
  });
}

export function onSettingsChanged(handler: () => void): Promise<() => void> {
  settingsListeners.add(handler);
  return Promise.resolve(() => {
    settingsListeners.delete(handler);
  });
}

export function onClipboardLinkDetected(
  _handler: (payload: ClipboardLinkDetectedPayload) => void,
): Promise<() => void> {
  return Promise.resolve(() => {});
}

export function onBrowserIntegrationChanged(handler: () => void): Promise<() => void> {
  browserIntegrationListeners.add(handler);
  return Promise.resolve(() => {
    browserIntegrationListeners.delete(handler);
  });
}

function browserDisplayName(browser: string): string {
  switch (browser) {
    case "chrome":
      return "Google Chrome";
    case "edge":
      return "Microsoft Edge";
    case "firefox":
      return "Mozilla Firefox";
    case "safari":
      return "Safari";
    case "brave":
      return "Brave";
    case "opera":
      return "Opera";
    case "vivaldi":
      return "Vivaldi";
    case "chromium":
      return "Chromium";
    default:
      return browser;
  }
}
