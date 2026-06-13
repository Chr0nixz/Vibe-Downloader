import { isTauriRuntime } from "@/lib/runtime";
import { createLogger } from "@/lib/logger";
import { parseAppError } from "@/lib/errors";
import type {
  AppSettings,
  BrowserCaptureSettings,
  BrowserCaptureSettingsInput,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  BrowserIntegrationUpdateInput,
  BatchImportResult,
  ClipboardLinkDetectedPayload,
  CreateTaskInput,
  HashVerificationState,
  ImportUrlsInput,
  CursorPageInput,
  ListTasksCursorInput,
  ListTasksCursorResult,
  ListTasksInput,
  ListTasksResult,
  RequestDiagnostic,
  ProbeTaskInput,
  ProbeTaskPayload,
  SegmentSummary,
  TaskEvent,
  TorrentRuntimeSnapshot,
  TrayMenuAction,
  ResolveTaskAttentionInput,
  TaskUpdatedPayload,
  UpdateSettingsInput,
} from "@/generated/bindings";
import type { Task } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";
import type { TaskProgressPayload } from "@/types/task-progress";

export const EVENT_TASK_PROGRESS = "task-progress";
export const EVENT_TASK_UPDATED = "task-updated";
export const EVENT_QUEUE_CHANGED = "queue-changed";
export const EVENT_SETTINGS_CHANGED = "settings-changed";
export const EVENT_BROWSER_INTEGRATION_CHANGED = "browser-integration-changed";
export const EVENT_TRAY_NEW_DOWNLOAD_REQUESTED = "tray-new-download-requested";
export const EVENT_TRAY_SETTINGS_REQUESTED = "tray-settings-requested";
export const EVENT_CLIPBOARD_LINK_DETECTED = "clipboard-link-detected";
export const canSeedMockTasks = !isTauriRuntime() || import.meta.env.DEV;

const log = createLogger("tauri");

type CommandResult<T, E> =
  | { status: "ok"; data: T }
  | { status: "error"; error: E };

function unwrapCommand<T, E>(result: CommandResult<T, E>): T {
  if (result.status === "ok") return result.data;
  throw parseAppError(result.error) ?? result.error;
}

async function runCommand<T>(name: string, run: () => Promise<CommandResult<T, unknown>>): Promise<T> {
  log.debug(`→ ${name}`);
  try {
    const data = unwrapCommand(await run());
    log.debug(`✓ ${name}`);
    return data;
  } catch (error) {
    log.error(`✗ ${name}`, error);
    throw error;
  }
}

async function loadNativeCommands() {
  const { commands } = await import("@/generated/bindings");
  return commands;
}

async function loadBrowserAdapter() {
  return import("@/lib/tauri-browser");
}

export async function listTasks(): Promise<Task[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTasks();
  }
  const commands = await loadNativeCommands();
  const tasks = await runCommand("listTasks", () => commands.listTasks());
  return tasks.map(normalizeTask);
}

export async function getTask(id: string): Promise<Task | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTask(id);
  }
  const commands = await loadNativeCommands();
  const task = await runCommand("getTask", () => commands.getTask(id));
  return task ? normalizeTask(task) : null;
}

export interface TaskPage {
  items: Task[];
  total: number;
  page: number;
  pageSize: number;
}

export interface TaskCursorPage {
  items: Task[];
  nextCursor: string | null;
  totalEstimate: number;
  filterOptions: ListTasksCursorResult["filterOptions"];
}

export interface CursorPage<T> {
  items: T[];
  nextCursor: string | null;
}

export async function listTasksCursor(input: ListTasksCursorInput): Promise<TaskCursorPage> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTasksCursor(input);
  }
  const commands = await loadNativeCommands();
  const result: ListTasksCursorResult = await runCommand("listTasksCursor", () =>
    commands.listTasksCursor(input),
  );
  return {
    items: result.items.map(normalizeTask),
    nextCursor: result.nextCursor,
    totalEstimate: Number(result.totalEstimate) || result.items.length,
    filterOptions: result.filterOptions,
  };
}

export async function listTasksPage(input: ListTasksInput): Promise<TaskPage> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTasksPage(input);
  }
  const commands = await loadNativeCommands();
  const result: ListTasksResult = await runCommand("listTasksPage", () =>
    commands.listTasksPage(input),
  );
  return {
    items: result.items.map(normalizeTask),
    total: Number(result.total) || 0,
    page: result.page,
    pageSize: result.pageSize,
  };
}

export async function listSegments(taskId: string, page = 0, pageSize = 100): Promise<TaskSegment[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listSegments(taskId, page, pageSize);
  }
  const commands = await loadNativeCommands();
  const segments = await runCommand("listSegments", () =>
    commands.listSegments({ taskId, page, pageSize }),
  );
  return segments.map(normalizeTaskSegment);
}

export async function listSegmentsPage(input: CursorPageInput): Promise<CursorPage<TaskSegment>> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listSegmentsPage(input);
  }
  const commands = await loadNativeCommands();
  const result = await runCommand("listSegmentsPage", () => commands.listSegmentsPage(input));
  return {
    items: result.items.map(normalizeTaskSegment),
    nextCursor: result.nextCursor,
  };
}

export async function getSegmentSummary(taskId: string): Promise<SegmentSummary> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getSegmentSummary(taskId);
  }
  const commands = await loadNativeCommands();
  return runCommand("getSegmentSummary", () => commands.getSegmentSummary(taskId));
}

export async function getTorrentRuntimeSnapshot(
  taskId: string,
): Promise<TorrentRuntimeSnapshot | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTorrentRuntimeSnapshot(taskId);
  }
  const commands = await loadNativeCommands();
  return runCommand("getTorrentRuntimeSnapshot", () =>
    commands.getTorrentRuntimeSnapshot(taskId),
  );
}

export async function listTaskEventsPage(input: CursorPageInput): Promise<CursorPage<TaskEvent>> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTaskEventsPage(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("listTaskEventsPage", () => commands.listTaskEventsPage(input));
}

export async function listTaskRequestsPage(
  input: CursorPageInput,
): Promise<CursorPage<RequestDiagnostic>> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTaskRequestsPage(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("listTaskRequestsPage", () => commands.listTaskRequestsPage(input));
}

export async function seedMockTasks(): Promise<Task[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).seedMockTasks();
  }
  if (!canSeedMockTasks) {
    throw new Error("Mock task reset is only available in development builds.");
  }
  const commands = await loadNativeCommands();
  const tasks = await runCommand("seedMockTasks", () => commands.seedMockTasks());
  return tasks.map(normalizeTask);
}

export async function getSettings(): Promise<AppSettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getSettings();
  }
  const commands = await loadNativeCommands();
  return runCommand("getSettings", () => commands.getSettings());
}

export async function updateSettings(input: UpdateSettingsInput): Promise<AppSettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateSettings(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("updateSettings", () => commands.updateSettings(input));
}

export async function openDirectoryPicker(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).openDirectoryPicker();
  }
  log.debug("→ openDirectoryPicker");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false });
  const path = typeof selected === "string" ? selected : null;
  log.debug("✓ openDirectoryPicker", path ?? "(canceled)");
  return path;
}

export interface PickedFile {
  path: string;
  name: string;
}

export async function openFilePicker(
  filters?: { name: string; extensions: string[] }[],
): Promise<PickedFile | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).openFilePicker(filters);
  }
  log.debug("→ openFilePicker");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: false,
    multiple: false,
    filters: filters ?? [
      { name: "Download files", extensions: ["torrent", "txt"] },
    ],
  });
  if (typeof selected !== "string") {
    log.debug("✓ openFilePicker (canceled)");
    return null;
  }
  const name = selected.replace(/[\\/]/g, "/").split("/").pop() ?? selected;
  log.debug("✓ openFilePicker", selected);
  return { path: selected, name };
}

export async function getBrowserIntegrationStatus(): Promise<BrowserIntegrationStatus> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getBrowserIntegrationStatus();
  }
  const commands = await loadNativeCommands();
  return runCommand("getBrowserIntegrationStatus", () => commands.getBrowserIntegrationStatus());
}

export async function installBrowserIntegration(
  input: BrowserIntegrationUpdateInput,
): Promise<BrowserIntegrationStatus> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).installBrowserIntegration(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("installBrowserIntegration", () => commands.installBrowserIntegration(input));
}

export async function uninstallBrowserIntegration(
  input: BrowserIntegrationUpdateInput,
): Promise<BrowserIntegrationStatus> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).uninstallBrowserIntegration(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("uninstallBrowserIntegration", () => commands.uninstallBrowserIntegration(input));
}

export async function exportBrowserExtensionPackages(): Promise<BrowserExtensionExportResult> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).exportBrowserExtensionPackages();
  }
  const commands = await loadNativeCommands();
  return runCommand("exportBrowserExtensionPackages", () =>
    commands.exportBrowserExtensionPackages(),
  );
}

export async function runTrayMenuAction(action: TrayMenuAction): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock tray menu action", action);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("runTrayMenuAction", () => commands.runTrayMenuAction(action));
}

export async function showFloatingStatusWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock show floating status window");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("showFloatingStatusWindow", () => commands.showFloatingStatusWindow());
}

export async function hideFloatingStatusWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock hide floating status window");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("hideFloatingStatusWindow", () => commands.hideFloatingStatusWindow());
}

export async function toggleFloatingStatusWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock toggle floating status window");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("toggleFloatingStatusWindow", () => commands.toggleFloatingStatusWindow());
}

export async function focusMainWindowFromFloating(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock focus main window from floating status");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("focusMainWindowFromFloating", () =>
    commands.focusMainWindowFromFloating(),
  );
}

export async function showTrayMenuAt(
  logicalX: number,
  logicalY: number,
): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock show tray menu at", logicalX, logicalY);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("showTrayMenuAt", () =>
    commands.showTrayMenuAt(logicalX, logicalY),
  );
}

export async function getBrowserCaptureSettings(): Promise<BrowserCaptureSettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getBrowserCaptureSettings();
  }
  const commands = await loadNativeCommands();
  return runCommand("getBrowserCaptureSettings", () => commands.getBrowserCaptureSettings());
}

export async function updateBrowserCaptureSettings(
  input: BrowserCaptureSettingsInput,
): Promise<BrowserCaptureSettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateBrowserCaptureSettings(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("updateBrowserCaptureSettings", () =>
    commands.updateBrowserCaptureSettings(input),
  );
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).createTask(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("createTask", () => commands.createTask(input)));
}

export async function importUrls(input: ImportUrlsInput): Promise<BatchImportResult> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).importUrls(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("importUrls", () => commands.importUrls(input));
}

export async function verifyTaskHash(id: string): Promise<HashVerificationState> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).verifyTaskHash(id);
  }
  const commands = await loadNativeCommands();
  return runCommand("verifyTaskHash", () => commands.verifyTaskHash(id));
}

export async function probeTask(input: ProbeTaskInput): Promise<ProbeTaskPayload> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).probeTask(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("probeTask", () => commands.probeTask(input));
}

export async function pauseTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).pauseTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("pauseTask", () => commands.pauseTask(id)));
}

export async function resumeTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).resumeTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("resumeTask", () => commands.resumeTask(id)));
}

export async function retryTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).retryTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("retryTask", () => commands.retryTask(id)));
}

export async function finishLiveRecording(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).finishLiveRecording(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(
    await runCommand("finishLiveRecording", () => commands.finishLiveRecording(id)),
  );
}

export async function resolveTaskAttention(input: ResolveTaskAttentionInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).resolveTaskAttention(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(
    await runCommand("resolveTaskAttention", () => commands.resolveTaskAttention(input)),
  );
}

export async function cancelTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).cancelTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("cancelTask", () => commands.cancelTask(id)));
}

export async function deleteTask(id: string, deleteFile = false): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).deleteTask(id, deleteFile);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("deleteTask", () => commands.deleteTask(id, deleteFile));
}

export async function openTaskFile(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).openTaskFile(id);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("openTaskFile", () => commands.openTaskFile(id));
}

export async function openTaskFolder(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).openTaskFolder(id);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("openTaskFolder", () => commands.openTaskFolder(id));
}

export function onTaskProgress(
  handler: (payload: TaskProgressPayload) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onTaskProgress(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<TaskProgressPayload>(EVENT_TASK_PROGRESS, (event) => {
      handler(event.payload);
    }).then((unlisten) => unlisten),
  );
}

export function onTaskUpdated(handler: (task: Task) => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onTaskUpdated(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<TaskUpdatedPayload>(EVENT_TASK_UPDATED, (event) => {
      handler(normalizeTask(event.payload.task));
    }).then((unlisten) => unlisten),
  );
}

export function onQueueChanged(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onQueueChanged(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_QUEUE_CHANGED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export function onSettingsChanged(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onSettingsChanged(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_SETTINGS_CHANGED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export function onTrayNewDownloadRequested(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return Promise.resolve(() => {});
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_TRAY_NEW_DOWNLOAD_REQUESTED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export function onTraySettingsRequested(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return Promise.resolve(() => {});
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_TRAY_SETTINGS_REQUESTED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export function onClipboardLinkDetected(
  handler: (payload: ClipboardLinkDetectedPayload) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) =>
      adapter.onClipboardLinkDetected(handler),
    );
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<ClipboardLinkDetectedPayload>(EVENT_CLIPBOARD_LINK_DETECTED, (event) => {
      handler(event.payload);
    }).then((unlisten) => unlisten),
  );
}

export function onBrowserIntegrationChanged(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) =>
      adapter.onBrowserIntegrationChanged(handler),
    );
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_BROWSER_INTEGRATION_CHANGED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export { isTauriRuntime };
