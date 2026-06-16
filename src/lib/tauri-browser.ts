import type {
  AppSettings,
  BatchImportResult,
  BrowserCaptureSettings,
  BrowserCaptureSettingsInput,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  BrowserIntegrationUpdateInput,
  ClipboardLinkDetectedPayload,
  CompletionActionRequestedPayload,
  CreateTaskInput,
  CursorPageInput,
  HashVerificationState,
  ImportUrlsInput,
  ListTasksCursorInput,
  ListTasksInput,
  ListTasksCursorResult,
  ProbeTaskInput,
  ProbeTaskPayload,
  FtpDirectoryProbe,
  SftpDirectoryProbe,
  WebDavDirectoryProbe,
  RequestDiagnostic,
  RecoveryAction,
  ResolveTaskAttentionInput,
  SegmentSummary,
  TaskProxySettings,
  TaskProxySettingsInput,
  TaskStatsSnapshot,
  TaskEvent,
  TorrentRuntimeSnapshot,
  UpdateTorrentFileSelectionInput,
  UpdateTorrentSeedingInput,
  UpdateSettingsInput,
  UpdateTaskTransferOptionsInput,
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

const LEGACY_HEALTH_SUMMARY_KEYS: Record<string, string> = {
  "Downloading": "taskDiagnostics.downloading",
  "Downloading steadily": "taskDiagnostics.downloadingSteadily",
  "Server limit detected": "taskDiagnostics.serverLimitDetected",
  "Network fluctuation, retrying": "taskDiagnostics.networkRetrying",
  "Resume unavailable": "taskDiagnostics.resumeUnavailable",
  "Remote file changed. Restart download to avoid corruption.":
    "taskDiagnostics.remoteChanged",
  "Completed": "taskDiagnostics.completed",
  "Waiting for network": "taskDiagnostics.waitingNetwork",
  "Disk write slower than network": "taskDiagnostics.diskWriteSlow",
  "Queued": "taskDiagnostics.queued",
  "Finishing HLS recording": "taskDiagnostics.finishingHls",
};

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
  autoResumeOnStartup: false,
  floatingWindowEnabled: false,
  clipboardMonitorEnabled: true,
  fontFamily: "source_han_sans_sc",
  accentColor: "blue",
  sidebarStripeEnabled: false,
  titlebarGradientEnabled: true,
  proxyMode: "off",
  proxyUrl: "",
  proxyNoProxy: "",
  proxyUsername: "",
  proxyPasswordSaved: false,
  scheduleDownloadWindowEnabled: false,
  scheduleDownloadWindowStart: "00:00",
  scheduleDownloadWindowEnd: "06:00",
  scheduleSpeedLimitWindowEnabled: false,
  scheduleSpeedLimitWindowStart: "18:00",
  scheduleSpeedLimitWindowEnd: "23:00",
  scheduleSpeedLimitBps: null,
  completionAction: "none",
  completionCountdownSeconds: 30,
};
const taskProxySettings = new Map<string, TaskProxySettings>();
let progressTimer: ReturnType<typeof setInterval> | undefined;
const progressListeners = new Set<BrowserListener>();
const taskUpdatedListeners = new Set<TaskUpdatedListener>();
const queueListeners = new Set<() => void>();
const settingsListeners = new Set<() => void>();
const browserIntegrationListeners = new Set<() => void>();
const completionActionListeners = new Set<(payload: CompletionActionRequestedPayload) => void>();
const browserIntegrationInstalled = new Set(["chrome", "edge"]);
let browserCaptureSettings: BrowserCaptureSettings =
  loadStoredBrowserCaptureSettings() ?? defaultBrowserCaptureSettings();

function nowIso(): string {
  return new Date().toISOString();
}

function mockErrorForHealthSummary(healthSummary: string | null): {
  errorMessage: string | null;
  recoveryActions: RecoveryAction[];
} {
  if (healthSummary === "taskDiagnostics.resumeUnavailable") {
    return {
      errorMessage: JSON.stringify({
        code: "resume_unavailable",
        message: healthSummary,
        recoverable: true,
        actions: ["restart", "open_folder"],
      }),
      recoveryActions: ["restart", "open_folder"],
    };
  }
  if (healthSummary === "taskDiagnostics.remoteChanged") {
    return {
      errorMessage: JSON.stringify({
        code: "remote_changed",
        message: healthSummary,
        recoverable: true,
        actions: ["restart", "open_folder"],
      }),
      recoveryActions: ["restart", "open_folder"],
    };
  }

  return {
    errorMessage: healthSummary,
    recoveryActions: [],
  };
}

function normalizeMockHealthSummary(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) return null;
  return LEGACY_HEALTH_SUMMARY_KEYS[trimmed] ?? trimmed;
}

function normalizeMockErrorMessage(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed) as { message?: unknown };
    if (typeof parsed.message === "string") {
      return JSON.stringify({
        ...parsed,
        message: normalizeMockHealthSummary(parsed.message) ?? parsed.message,
      });
    }
  } catch {
    /* legacy plain string */
  }
  return LEGACY_HEALTH_SUMMARY_KEYS[trimmed] ?? trimmed;
}

function normalizeStoredMockTask(rawTask: Task): Task {
  const task = normalizeTask(rawTask);
  const healthSummary = normalizeMockHealthSummary(task.healthSummary);
  const derivedError = mockErrorForHealthSummary(healthSummary);
  const isAttentionState =
    task.status === "failed" || task.status === "needs_attention";

  return {
    ...task,
    healthSummary,
    errorMessage:
      isAttentionState && derivedError.errorMessage
        ? derivedError.errorMessage
        : normalizeMockErrorMessage(task.errorMessage),
    recoveryActions:
      isAttentionState && task.recoveryActions.length === 0
        ? derivedError.recoveryActions
        : task.recoveryActions,
  };
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
    const parsed = JSON.parse(raw) as unknown[];
    return Array.isArray(parsed)
      ? parsed.map((task) => normalizeStoredMockTask(task as Task))
      : null;
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
        autoResumeOnStartup:
          typeof parsed.autoResumeOnStartup === "boolean"
            ? parsed.autoResumeOnStartup
            : false,
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
        sidebarStripeEnabled: Boolean(parsed.sidebarStripeEnabled ?? false),
        titlebarGradientEnabled: Boolean(parsed.titlebarGradientEnabled ?? true),
        proxyMode:
          parsed.proxyMode === "system" || parsed.proxyMode === "custom" ? parsed.proxyMode : "off",
        proxyUrl: typeof parsed.proxyUrl === "string" ? parsed.proxyUrl : "",
        proxyNoProxy: typeof parsed.proxyNoProxy === "string" ? parsed.proxyNoProxy : "",
        proxyUsername: typeof parsed.proxyUsername === "string" ? parsed.proxyUsername : "",
        proxyPasswordSaved: Boolean(parsed.proxyPasswordSaved),
        scheduleDownloadWindowEnabled: Boolean(parsed.scheduleDownloadWindowEnabled),
        scheduleDownloadWindowStart:
          typeof parsed.scheduleDownloadWindowStart === "string"
            ? parsed.scheduleDownloadWindowStart
            : "00:00",
        scheduleDownloadWindowEnd:
          typeof parsed.scheduleDownloadWindowEnd === "string"
            ? parsed.scheduleDownloadWindowEnd
            : "06:00",
        scheduleSpeedLimitWindowEnabled: Boolean(parsed.scheduleSpeedLimitWindowEnabled),
        scheduleSpeedLimitWindowStart:
          typeof parsed.scheduleSpeedLimitWindowStart === "string"
            ? parsed.scheduleSpeedLimitWindowStart
            : "18:00",
        scheduleSpeedLimitWindowEnd:
          typeof parsed.scheduleSpeedLimitWindowEnd === "string"
            ? parsed.scheduleSpeedLimitWindowEnd
            : "23:00",
        scheduleSpeedLimitBps:
          typeof parsed.scheduleSpeedLimitBps === "string"
            ? parsed.scheduleSpeedLimitBps
            : null,
        completionAction:
          parsed.completionAction === "exit_app" || parsed.completionAction === "shutdown"
            ? parsed.completionAction
            : "none",
        completionCountdownSeconds:
          typeof parsed.completionCountdownSeconds === "number"
            ? Math.min(300, Math.max(5, parsed.completionCountdownSeconds))
            : 30,
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
    fileExtensions: ["zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "pdf", "m3u8", "mpd", "meta4", "metalink"],
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
  const mockError = needsError
    ? mockErrorForHealthSummary(healthSummary)
    : { errorMessage: null, recoveryActions: [] };
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
    taskSpeedLimitBps: null,
    priority: "normal",
    queuePosition: "0",
    categoryKey: null,
    obeySchedule: true,
    healthSummary,
    errorMessage: mockError.errorMessage,
    errorCode: null,
    recoveryActions: mockError.recoveryActions,
    retryAfterAt: null,
    failureCategory: needsError ? "other" : null,
    expectedHashSha256: null,
    actualHashSha256: null,
    hashStatus: "not_requested",
    hashError: null,
    hashVerifiedAt: null,
    checksums: [],
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
    browserTask("mock-ubuntu", "ubuntu-24.04.iso", "https://releases.ubuntu.com/noble/ubuntu-24.04-desktop-amd64.iso", "releases.ubuntu.com", "downloading", 4_200_000_000, 1_680_000_000, 8, 48_500_000, "taskDiagnostics.downloadingSteadily", now),
    browserTask("mock-node", "node-v22.pkg", "https://nodejs.org/dist/v22.0.0/node-v22.0.0.pkg", "nodejs.org", "downloading", 80_000_000, 52_000_000, 4, 12_400_000, "taskDiagnostics.serverLimitDetected", now),
    browserTask("mock-rust", "rust-docs.pdf", "https://doc.rust-lang.org/book.pdf", "doc.rust-lang.org", "paused", 12_000_000, 4_800_000, 0, 0, null, now),
    browserTask("mock-game", "game-patch.zip", "https://cdn.example.com/patches/season-12.zip", "cdn.example.com", "queued", 2_400_000_000, 0, 0, 0, null, now),
    browserTask("mock-dataset", "dataset.tar.gz", "https://data.example.org/ml/dataset.tar.gz", "data.example.org", "retrying", 900_000_000, 120_000_000, 2, 3_200_000, "taskDiagnostics.networkRetrying", now),
    browserTask("mock-driver", "driver-setup.exe", "https://vendor.example.net/drivers/setup.exe", "vendor.example.net", "failed", 350_000_000, 89_000_000, 0, 0, "taskDiagnostics.resumeUnavailable", now),
    browserTask("mock-llm", "llm-weights.safetensors", "https://models.example.ai/weights/v3.safetensors", "models.example.ai", "needs_attention", 8_000_000_000, 2_100_000_000, 0, 0, "taskDiagnostics.remoteChanged", now),
    browserTask("mock-arch", "archlinux.iso", "https://mirror.archlinux.org/iso/latest/archlinux-x86_64.iso", "mirror.archlinux.org", "completed", 1_300_000_000, 1_300_000_000, 0, 0, "taskDiagnostics.completed", now),
    browserTask("mock-fonts", "fonts-bundle.zip", "https://github.com/google/fonts/archive/refs/heads/main.zip", "github.com", "waiting_network", 220_000_000, 45_000_000, 0, 0, "taskDiagnostics.waitingNetwork", now),
    browserTask("mock-vscode", "vscode.deb", "https://code.visualstudio.com/sha/download?build=stable&os=linux-deb-x64", "code.visualstudio.com", "downloading", 95_000_000, 71_000_000, 2, 8_900_000, "taskDiagnostics.diskWriteSlow", now),
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
      connectionCount: task.supportsParallel
        ? Math.min(
            settings.maxConnectionsPerHost,
            task.connectionCount > 0 ? task.connectionCount : settings.segmentCount,
          )
        : 1,
      healthSummary: "taskDiagnostics.downloading",
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
      healthSummary: completed ? "taskDiagnostics.completed" : task.healthSummary,
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

export async function getTaskStats(): Promise<TaskStatsSnapshot> {
  const activeTasks = tasks.filter(
    (task) => task.status === "downloading" || task.status === "retrying",
  );
  const fallback = tasks.find((task) =>
    ["queued", "paused", "failed", "needs_attention", "waiting_network"].includes(task.status),
  );
  const featuredTask =
    activeTasks.reduce<Task | null>(
      (best, task) => (!best || task.speedBps > best.speedBps ? task : best),
      null,
    ) ?? fallback ?? null;

  return {
    all: String(tasks.length),
    active: String(activeTasks.length),
    queued: String(tasks.filter((task) => task.status === "queued").length),
    paused: String(tasks.filter((task) =>
      task.status === "paused" ||
      task.status === "queued" ||
      task.status === "waiting_network",
    ).length),
    completed: String(tasks.filter((task) => task.status === "completed").length),
    failed: String(tasks.filter((task) => task.status === "failed" || task.status === "needs_attention").length),
    totalSpeed: String(activeTasks.reduce((sum, task) => sum + task.speedBps, 0)),
    totalDownloaded: String(activeTasks.reduce((sum, task) => sum + task.downloadedBytes, 0)),
    totalBytes: String(activeTasks.reduce((sum, task) => sum + task.totalSize, 0)),
    featuredTaskId: featuredTask?.id ?? null,
  };
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
    pieceCount: String(Math.max(1, Math.ceil(task.totalSize / (4 * 1024 * 1024)))),
    pieceBitfieldBase64: null,
    peerCount: live ? "12" : "0",
    seedCount: live ? "4" : "0",
    dhtStatus: JSON.stringify({ routing_table_size: live ? 128 : 0 }),
    trackers: [{ url: "udp://tracker.example:6969/announce", status: "configured", lastError: null }],
    uploadBytes: String(uploaded),
    uploadSpeedBps: String(uploadSpeedBps),
    ratio: task.downloadedBytes > 0 ? uploaded / task.downloadedBytes : 0,
    seedingEnabled: false,
    seedingState: "disabled",
    lastErrorCode: null,
    lastErrorMessage: null,
    updatedAt: task.updatedAt,
  };
}

export async function getTaskProxySettings(taskId: string): Promise<TaskProxySettings> {
  return (
    taskProxySettings.get(taskId) ?? {
      taskId,
      mode: "inherit",
      proxyUrl: "",
      proxyUsername: "",
      proxyPasswordSaved: false,
      noProxy: "",
    }
  );
}

export async function updateTaskProxySettings(
  input: TaskProxySettingsInput,
): Promise<TaskProxySettings> {
  const next: TaskProxySettings = {
    taskId: input.taskId,
    mode: input.mode,
    proxyUrl: input.proxyUrl ?? "",
    proxyUsername: input.proxyUsername ?? "",
    proxyPasswordSaved: input.clearProxyPassword ? false : Boolean(input.proxyPassword),
    noProxy: input.noProxy ?? "",
  };
  taskProxySettings.set(input.taskId, next);
  return next;
}

export async function updateTorrentFileSelection(
  input: UpdateTorrentFileSelectionInput,
): Promise<Task> {
  const task = tasks.find((entry) => entry.id === input.taskId);
  if (!task) throw new Error("Task not found.");
  const selected = new Set(input.selectedFilePaths);
  task.files = task.files.map((file) => ({
    ...file,
    selected: selected.has(file.relativePath),
  }));
  task.status = "queued";
  task.errorMessage = null;
  task.errorCode = null;
  task.updatedAt = nowIso();
  persistTasks();
  emitTaskUpdated(task);
  scheduleBrowserQueue();
  return task;
}

export async function updateTorrentSeeding(input: UpdateTorrentSeedingInput): Promise<Task> {
  const task = tasks.find((entry) => entry.id === input.taskId);
  if (!task) throw new Error("Task not found.");
  task.updatedAt = nowIso();
  persistTasks();
  emitTaskUpdated(task);
  return task;
}

export async function probeFtpDirectory(url: string): Promise<FtpDirectoryProbe> {
  const base = url.replace(/\/?$/, "/");
  return {
    inputUrl: url,
    directoryUrl: base,
    currentDirectory: "/",
    entries: [
      { name: "example.bin", raw: "-rw-r--r-- 1 user group 1024 example.bin", probableFileUrl: `${base}example.bin` },
      { name: "nested", raw: "drwxr-xr-x 1 user group 0 nested", probableFileUrl: null },
    ],
    diagnostics: ["Mock FTP directory probe"],
  };
}

export async function probeSftpDirectory(url: string): Promise<SftpDirectoryProbe> {
  const base = url.replace(/\/?$/, "/");
  return {
    inputUrl: url,
    directoryUrl: base,
    currentDirectory: "/",
    entries: [
      { name: "backup.tar.zst", raw: "-rw-r--r-- 1 user group 1048576 backup.tar.zst", probableFileUrl: `${base}backup.tar.zst` },
      { name: "logs", raw: "drwxr-xr-x 1 user group 0 logs", probableFileUrl: null },
    ],
    diagnostics: ["Mock SFTP directory probe"],
  };
}

export async function probeWebdavDirectory(url: string): Promise<WebDavDirectoryProbe> {
  const base = url.replace(/\/?$/, "/");
  return {
    inputUrl: url,
    directoryUrl: base,
    entries: [
      {
        name: "release-video.mp4",
        href: `${base}release-video.mp4`,
        isCollection: false,
        size: "73400320",
        contentType: "video/mp4",
        probableFileUrl: `${base}release-video.mp4`,
      },
      {
        name: "archive",
        href: `${base}archive/`,
        isCollection: true,
        size: null,
        contentType: null,
        probableFileUrl: null,
      },
    ],
    diagnostics: ["Mock WebDAV PROPFIND returned 207 Multi-Status"],
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
        ifRangeHeader: task.supportsParallel ? (task.etag ?? task.lastModified) : null,
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
  if (task.errorCode?.startsWith("dash_")) return "dash";
  if (task.errorCode?.startsWith("webdav_")) return "webdav";
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
    autoResumeOnStartup:
      input.autoResumeOnStartup ?? settings.autoResumeOnStartup,
    floatingWindowEnabled:
      input.floatingWindowEnabled ?? settings.floatingWindowEnabled,
    clipboardMonitorEnabled:
      input.clipboardMonitorEnabled ?? settings.clipboardMonitorEnabled,
    fontFamily: input.fontFamily ?? settings.fontFamily,
    accentColor: input.accentColor ?? settings.accentColor,
    sidebarStripeEnabled:
      input.sidebarStripeEnabled ?? settings.sidebarStripeEnabled,
    titlebarGradientEnabled:
      input.titlebarGradientEnabled ?? settings.titlebarGradientEnabled,
    proxyMode: input.proxyMode ?? settings.proxyMode,
    proxyUrl: input.proxyUrl ?? settings.proxyUrl,
    proxyNoProxy: input.proxyNoProxy ?? settings.proxyNoProxy,
    proxyUsername: input.proxyUsername ?? settings.proxyUsername,
    proxyPasswordSaved: input.clearProxyPassword
      ? false
      : input.proxyPassword
        ? true
        : settings.proxyPasswordSaved,
    scheduleDownloadWindowEnabled:
      input.scheduleDownloadWindowEnabled ?? settings.scheduleDownloadWindowEnabled,
    scheduleDownloadWindowStart:
      input.scheduleDownloadWindowStart ?? settings.scheduleDownloadWindowStart,
    scheduleDownloadWindowEnd:
      input.scheduleDownloadWindowEnd ?? settings.scheduleDownloadWindowEnd,
    scheduleSpeedLimitWindowEnabled:
      input.scheduleSpeedLimitWindowEnabled ?? settings.scheduleSpeedLimitWindowEnabled,
    scheduleSpeedLimitWindowStart:
      input.scheduleSpeedLimitWindowStart ?? settings.scheduleSpeedLimitWindowStart,
    scheduleSpeedLimitWindowEnd:
      input.scheduleSpeedLimitWindowEnd ?? settings.scheduleSpeedLimitWindowEnd,
    scheduleSpeedLimitBps:
      input.scheduleSpeedLimitBps === null || input.scheduleSpeedLimitBps === undefined
        ? settings.scheduleSpeedLimitBps
        : normalizeSpeedLimit(input.scheduleSpeedLimitBps),
    completionAction: input.completionAction ?? settings.completionAction,
    completionCountdownSeconds: Math.min(
      300,
      Math.max(5, input.completionCountdownSeconds ?? settings.completionCountdownSeconds),
    ),
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
  const duplicate = duplicateTaskForProbe(normalizedUrl, probe);
  if (duplicate && !input.allowDuplicate) {
    throw JSON.stringify({
      code: "duplicate_task",
      message: `A task for this download already exists: ${duplicate.fileName}`,
      recoverable: true,
      actions: [],
    });
  }
  const fileName = input.fileName?.trim() || probe.fileName || "download.bin";
  const isSelectableMultiFile =
    (probe.protocol === "bt" || probe.protocol === "magnet" || probe.protocol === "metalink") &&
    probe.files.length > 1;
  if (isSelectableMultiFile && input.selectedFilePaths && input.selectedFilePaths.length === 0) {
    throw new Error("Select at least one file to download.");
  }
  const selectedFilePaths = new Set(
    isSelectableMultiFile ? (input.selectedFilePaths ?? []).filter(Boolean) : [],
  );
  if (selectedFilePaths.size > 0) {
    const availablePaths = new Set(probe.files.map((file) => file.relativePath));
    const unknown = Array.from(selectedFilePaths).filter((path) => !availablePaths.has(path));
    if (unknown.length > 0) {
      throw new Error(`Unknown file selection: ${unknown[0]}`);
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
    supportsResume: probe.capabilities.supportsResume,
    supportsParallel: probe.capabilities.supportsParallel,
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

export async function updateTaskTransferOptions(
  input: UpdateTaskTransferOptionsInput,
): Promise<Task> {
  const task = tasks.find((item) => item.id === input.id);
  if (!task) throw new Error(`Task not found: ${input.id}`);
  return updateTask(input.id, {
    taskSpeedLimitBps: input.taskSpeedLimitBps,
    priority: input.priority ?? task.priority,
    queuePosition: input.queuePosition ?? task.queuePosition,
    categoryKey: input.categoryKey,
  });
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
    const existingTask = !duplicate && probe ? duplicateTaskForProbe(normalized, probe) : null;
    const isDuplicate = duplicate || Boolean(existingTask);
    const task =
      !isDuplicate && input.create
        ? await createTask({
            url: normalized,
            saveDir: input.saveDir,
            fileName: probe?.fileName ?? null,
            expectedHashSha256: null,
            taskSpeedLimitBps: null,
            priority: null,
            categoryKey: null,
            probeSnapshot: probe,
            selectedFilePaths: null,
            allowDuplicate: false,
          })
        : null;
    items.push({
      inputUrl: raw,
      normalizedUrl: normalized,
      duplicate: isDuplicate,
      valid: !duplicate,
      fileName: existingTask?.fileName ?? probe?.fileName ?? null,
      totalSize: existingTask ? String(existingTask.totalSize) : (probe?.totalSize ?? null),
      contentType: existingTask?.contentType ?? probe?.contentType ?? null,
      supportsResume: existingTask?.supportsResume ?? probe?.capabilities.supportsResume ?? false,
      errorMessage: duplicate
        ? "Duplicate URL in this import."
        : existingTask
          ? `Task already exists: ${existingTask.fileName}`
          : null,
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

function duplicateTaskForProbe(url: string, probe: ProbeTaskPayload): Task | null {
  return (
    tasks.find((task) => {
      if (task.url === url || task.finalUrl === url) return true;
      if (task.url === probe.finalUrl || task.finalUrl === probe.finalUrl) return true;
      return probe.sourceKey.startsWith("bt:") && task.sourceKey === probe.sourceKey;
    }) ?? null
  );
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
      hlsVariants: [],
      probedAt: nowIso(),
    };
  }

  const parsed = safeUrl(input.url);
  const protocol = parsed?.protocol.replace(":", "") || "http";
  const host = parsed?.host ?? "example.com";
  const path = parsed?.pathname ?? "";
  if (/\.meta4$|\.metalink$/i.test(path)) {
    const sourceName = fileNameFromRelativePath(path) || "download.meta4";
    return {
      inputUrl: input.url,
      finalUrl: input.url,
      fileName: sourceName.replace(/\.(meta4|metalink)$/i, ""),
      protocol: "metalink",
      taskKind: "manifest",
      capabilities: {
        supportsResume: true,
        supportsParallel: true,
        supportsMultiFile: true,
      },
      files: [
        {
          relativePath: "package/app-installer.exe",
          size: "73400320",
          contentType: "application/octet-stream",
        },
        {
          relativePath: "package/checksums.txt",
          size: "4096",
          contentType: "text/plain",
        },
      ],
      totalSize: "73404416",
      sourceKey: `${protocol}://${host}`,
      contentType: "application/metalink4+xml",
      etag: null,
      lastModified: null,
      hlsVariants: [],
      probedAt: nowIso(),
    };
  }
  if ((protocol === "http" || protocol === "https") && /\.m3u8$/i.test(path)) {
    const sourceName = fileNameFromRelativePath(path) || "playlist.m3u8";
    const fileName = sourceName.replace(/\.m3u8$/i, ".mp4");
    return {
      inputUrl: input.url,
      finalUrl: input.url,
      fileName,
      protocol: "hls",
      taskKind: "manifest",
      capabilities: {
        supportsResume: true,
        supportsParallel: true,
        supportsMultiFile: false,
      },
      files: [
        {
          relativePath: fileName,
          size: "0",
          contentType: "video/mp4",
        },
      ],
      totalSize: "0",
      sourceKey: `${protocol}://${host}`,
      contentType: "application/vnd.apple.mpegurl",
      etag: null,
      lastModified: null,
      hlsVariants: [
        {
          uri: input.url,
          bandwidth: "6000000",
          resolution: "1920x1080",
          codecs: "avc1.640028,mp4a.40.2",
          selected: true,
        },
      ],
      probedAt: nowIso(),
    };
  }
  if ((protocol === "http" || protocol === "https" || protocol === "file") && /\.mpd$/i.test(path)) {
    const sourceName = fileNameFromRelativePath(path) || "manifest.mpd";
    const fileName = sourceName.replace(/\.mpd$/i, ".mp4");
    return {
      inputUrl: input.url,
      finalUrl: input.url,
      fileName,
      protocol: "dash",
      taskKind: "manifest",
      capabilities: {
        supportsResume: false,
        supportsParallel: true,
        supportsMultiFile: false,
      },
      files: [
        {
          relativePath: fileName,
          size: "0",
          contentType: "video/mp4",
        },
      ],
      totalSize: "0",
      sourceKey: `${protocol}://${host}`,
      contentType: "application/dash+xml",
      etag: null,
      lastModified: null,
      hlsVariants: [],
      probedAt: nowIso(),
    };
  }
  const isSftp = protocol === "sftp";
  const fileName = fileNameFromRelativePath(decodeUrlPath(path));
  const sourceKey = isSftp && parsed
    ? `sftp://${parsed.hostname}:${parsed.port || "22"}`
    : `${protocol}://${host}`;
  return {
    inputUrl: input.url,
    finalUrl: input.url,
    fileName,
    protocol,
    taskKind: "single_file",
    capabilities: {
      supportsResume: true,
      supportsParallel: !isSftp,
      supportsMultiFile: false,
    },
    files: [
      {
        relativePath: fileName,
        size: "1048576",
        contentType: "application/octet-stream",
      },
    ],
    totalSize: "1048576",
    sourceKey,
    contentType: "application/octet-stream",
    etag: isSftp ? null : "\"mock-etag\"",
    lastModified: nowIso(),
    hlsVariants: [],
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

function decodeUrlPath(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
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
    healthSummary: "taskDiagnostics.queued",
  });
  logTaskEvent(id, "resumed");
  scheduleBrowserQueue();
  return tasks.find((entry) => entry.id === next.id) ?? next;
}

export async function retryTask(id: string): Promise<Task> {
  logTaskEvent(id, "retrying");
  return resumeTask(id);
}

export async function finishLiveRecording(id: string): Promise<Task> {
  const task = tasks.find((entry) => entry.id === id);
  if (!task) throw new Error(`Task not found: ${id}`);
  if (task.protocol !== "hls") throw new Error("Only HLS live recordings can be finished.");
  const next = updateTask(id, {
    healthSummary: "taskDiagnostics.finishingHls",
  });
  logTaskEvent(id, "hls_finish_requested");
  return next;
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
      healthSummary: "taskDiagnostics.queued",
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

export function onCompletionActionRequested(
  handler: (payload: CompletionActionRequestedPayload) => void,
): Promise<() => void> {
  completionActionListeners.add(handler);
  return Promise.resolve(() => {
    completionActionListeners.delete(handler);
  });
}

export async function requestSystemShutdown(): Promise<void> {
  log.info("mock system shutdown requested");
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
