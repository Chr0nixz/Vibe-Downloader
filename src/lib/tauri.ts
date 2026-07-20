import type {
  AppSettings,
  BatchImportResult,
  BrowserCaptureSettings,
  BrowserCaptureSettingsInput,
  BrowserExtensionExportResult,
  BrowserIntegrationStatus,
  BrowserIntegrationUpdateInput,
  ChecksumAlgorithm,
  ClassificationRule,
  ClassificationRuleInput,
  ClipboardLinkDetectedPayload,
  CompletionActionRequestedPayload,
  CreateTaskInput,
  CursorPageInput,
  DirectoryProbeInput,
  DiskSpaceInfo,
  FtpDirectoryProbe,
  HashVerificationState,
  ImportUrlsInput,
  ListTasksCursorInput,
  ListTasksCursorResult,
  ListTasksInput,
  ListTasksResult,
  MetalinkMirrorView,
  ProbePhasePayload,
  ProbeTaskInput,
  ProbeTaskPayload,
  QueueChangedPayload,
  RequestDiagnostic,
  ResolveTaskAttentionInput,
  SchedulerSnapshot,
  SegmentSummary,
  SftpDirectoryProbe,
  SftpKnownHost,
  StartupStatus,
  SystemFileIcon,
  TaskEvent,
  TaskProxySettings,
  TaskProxySettingsInput,
  TaskStatsSnapshot,
  TaskUpdatedPayload,
  TorrentRuntimeSnapshot,
  TrayMenuAction,
  UpdateSettingsInput,
  UpdateTaskTransferOptionsInput,
  UpdateTorrentFileSelectionInput,
  UpdateTorrentSeedingInput,
  WebDavDirectoryProbe,
} from "@/generated/bindings";
import { parseAppError } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { isTauriRuntime } from "@/lib/runtime";
import type { Task } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskProgressPayload } from "@/types/task-progress";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";

export const EVENT_TASK_PROGRESS = "task-progress";
export const EVENT_TASK_UPDATED = "task-updated";
export const EVENT_QUEUE_CHANGED = "queue-changed";
export const EVENT_SETTINGS_CHANGED = "settings-changed";
export const EVENT_BROWSER_INTEGRATION_CHANGED = "browser-integration-changed";
export const EVENT_TRAY_NEW_DOWNLOAD_REQUESTED = "tray-new-download-requested";
export const EVENT_TRAY_SETTINGS_REQUESTED = "tray-settings-requested";
export const EVENT_CLIPBOARD_LINK_DETECTED = "clipboard-link-detected";
export const EVENT_COMPLETION_ACTION_REQUESTED = "completion-action-requested";
export const EVENT_PROBE_PHASE = "probe-phase";
export const EVENT_SHUTTING_DOWN = "app://shutting-down";
export const canSeedMockTasks = !isTauriRuntime() || import.meta.env.DEV;

const log = createLogger("tauri");

type CommandResult<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

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

export async function getStartupStatus(): Promise<StartupStatus> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).getStartupStatus();
  const commands = await loadNativeCommands();
  return runCommand("getStartupStatus", () => commands.getStartupStatus());
}

export async function openDatabaseRecoveryFolder(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).openDatabaseRecoveryFolder();
  const commands = await loadNativeCommands();
  await runCommand("openDatabaseRecoveryFolder", () => commands.openDatabaseRecoveryFolder());
}

export async function resetDatabaseForRecovery(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).resetDatabaseForRecovery();
  const commands = await loadNativeCommands();
  await runCommand("resetDatabaseForRecovery", () => commands.resetDatabaseForRecovery());
}

export async function openStartupLogFolder(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).openStartupLogFolder();
  const commands = await loadNativeCommands();
  await runCommand("openStartupLogFolder", () => commands.openStartupLogFolder());
}

export async function openStartupDataFolder(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).openStartupDataFolder();
  const commands = await loadNativeCommands();
  await runCommand("openStartupDataFolder", () => commands.openStartupDataFolder());
}

export async function retryStartupInit(): Promise<void> {
  if (!isTauriRuntime()) return (await loadBrowserAdapter()).retryStartupInit();
  const commands = await loadNativeCommands();
  await runCommand("retryStartupInit", () => commands.retryStartupInit());
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

export async function listTasksByIds(ids: string[]): Promise<Task[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTasksByIds(ids);
  }
  const commands = await loadNativeCommands();
  const tasks = await runCommand("listTasksByIds", () => commands.listTasksByIds(ids));
  return tasks.map(normalizeTask);
}

export async function getTaskStats(): Promise<TaskStatsSnapshot> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTaskStats();
  }
  const commands = await loadNativeCommands();
  return runCommand("getTaskStats", () => commands.getTaskStats());
}

export async function getSchedulerSnapshot(taskIds: string[]): Promise<SchedulerSnapshot> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getSchedulerSnapshot(taskIds);
  }
  const commands = await loadNativeCommands();
  return runCommand("getSchedulerSnapshot", () => commands.getSchedulerSnapshot(taskIds));
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
  minimumTotal: number;
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
  const result: ListTasksCursorResult = await runCommand("listTasksCursor", () => commands.listTasksCursor(input));
  return {
    items: result.items.map(normalizeTask),
    nextCursor: result.nextCursor,
    minimumTotal: Number(result.minimumTotal) || result.items.length,
    filterOptions: result.filterOptions,
  };
}

export async function listTasksPage(input: ListTasksInput): Promise<TaskPage> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTasksPage(input);
  }
  const commands = await loadNativeCommands();
  const result: ListTasksResult = await runCommand("listTasksPage", () => commands.listTasksPage(input));
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
  const segments = await runCommand("listSegments", () => commands.listSegments({ taskId, page, pageSize }));
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

export async function getTorrentRuntimeSnapshot(taskId: string): Promise<TorrentRuntimeSnapshot | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTorrentRuntimeSnapshot(taskId);
  }
  const commands = await loadNativeCommands();
  return runCommand("getTorrentRuntimeSnapshot", () => commands.getTorrentRuntimeSnapshot(taskId));
}

export async function getTaskProxySettings(taskId: string): Promise<TaskProxySettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTaskProxySettings(taskId);
  }
  const commands = await loadNativeCommands();
  return runCommand("getTaskProxySettings", () => commands.getTaskProxySettings(taskId));
}

export async function updateTaskProxySettings(input: TaskProxySettingsInput): Promise<TaskProxySettings> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateTaskProxySettings(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("updateTaskProxySettings", () => commands.updateTaskProxySettings(input));
}

export async function updateTorrentFileSelection(input: UpdateTorrentFileSelectionInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateTorrentFileSelection(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(
    await runCommand("updateTorrentFileSelection", () => commands.updateTorrentFileSelection(input)),
  );
}

export async function updateTorrentSeeding(input: UpdateTorrentSeedingInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateTorrentSeeding(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("updateTorrentSeeding", () => commands.updateTorrentSeeding(input)));
}

export async function listTaskEventsPage(input: CursorPageInput): Promise<CursorPage<TaskEvent>> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTaskEventsPage(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("listTaskEventsPage", () => commands.listTaskEventsPage(input));
}

export async function listTaskRequestsPage(input: CursorPageInput): Promise<CursorPage<RequestDiagnostic>> {
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

export async function listSftpKnownHosts(): Promise<SftpKnownHost[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listSftpKnownHosts();
  }
  const commands = await loadNativeCommands();
  return runCommand("listSftpKnownHosts", () => commands.listSftpKnownHosts());
}

export async function forgetSftpKnownHost(host: string, port: number): Promise<boolean> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).forgetSftpKnownHost(host, port);
  }
  const commands = await loadNativeCommands();
  return runCommand("forgetSftpKnownHost", () => commands.forgetSftpKnownHost(host, port));
}

export async function probeFfmpegVersion(path?: string | null): Promise<string> {
  if (!isTauriRuntime()) {
    // Browser preview: report missing so the Settings UI shows the "not detected" badge.
    throw new Error("ffmpeg is not available in browser preview mode.");
  }
  const commands = await loadNativeCommands();
  return runCommand("probeFfmpegVersion", () => commands.probeFfmpegVersion(path ?? null));
}

export async function probeFtpDirectory(input: DirectoryProbeInput): Promise<FtpDirectoryProbe> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).probeFtpDirectory(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("probeFtpDirectory", () => commands.probeFtpDirectory(input));
}

export async function probeSftpDirectory(input: DirectoryProbeInput): Promise<SftpDirectoryProbe> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).probeSftpDirectory(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("probeSftpDirectory", () => commands.probeSftpDirectory(input));
}

export async function probeWebdavDirectory(input: DirectoryProbeInput): Promise<WebDavDirectoryProbe> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).probeWebdavDirectory(input);
  }
  const commands = await loadNativeCommands();
  return runCommand("probeWebdavDirectory", () => commands.probeWebdavDirectory(input));
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

export async function openFilePicker(filters?: { name: string; extensions: string[] }[]): Promise<PickedFile | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).openFilePicker(filters);
  }
  log.debug("→ openFilePicker");
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: false,
    multiple: false,
    filters: filters ?? [{ name: "Download files", extensions: ["torrent", "meta4", "metalink", "txt"] }],
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

export async function createAppBackup(destinationPath: string) {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).createAppBackup(destinationPath);
  }
  const commands = await loadNativeCommands();
  return runCommand("createAppBackup", () => commands.createAppBackup(destinationPath));
}

export async function validateAppBackup(backupPath: string) {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).validateAppBackup(backupPath);
  }
  const commands = await loadNativeCommands();
  return runCommand("validateAppBackup", () => commands.validateAppBackup(backupPath));
}

export async function restoreAppBackup(backupPath: string) {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).restoreAppBackup(backupPath);
  }
  const commands = await loadNativeCommands();
  return runCommand("restoreAppBackup", () => commands.restoreAppBackup(backupPath));
}

export type BackupCreateResult = Awaited<ReturnType<typeof createAppBackup>>;
export type BackupValidateResult = Awaited<ReturnType<typeof validateAppBackup>>;
export type BackupRestoreResult = Awaited<ReturnType<typeof restoreAppBackup>>;

export async function exportBrowserExtensionPackages(): Promise<BrowserExtensionExportResult> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).exportBrowserExtensionPackages();
  }
  const commands = await loadNativeCommands();
  return runCommand("exportBrowserExtensionPackages", () => commands.exportBrowserExtensionPackages());
}

export async function runTrayMenuAction(action: TrayMenuAction): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock tray menu action", action);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("runTrayMenuAction", () => commands.runTrayMenuAction(action));
}

export async function requestSystemShutdown(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock request system shutdown");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("requestSystemShutdown", () => commands.requestSystemShutdown());
}

export async function requestSystemSleep(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock request system sleep");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("requestSystemSleep", () => commands.requestSystemSleep());
}

export async function requestSystemHibernate(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock request system hibernate");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("requestSystemHibernate", () => commands.requestSystemHibernate());
}

export async function requestLockScreen(): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock request lock screen");
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("requestLockScreen", () => commands.requestLockScreen());
}

export async function queryDiskSpace(path: string): Promise<DiskSpaceInfo> {
  if (!isTauriRuntime()) {
    // Browser preview mock: report a generous virtual disk so the disk-space
    // recovery action shows something useful in dev.
    return {
      path,
      total_bytes: String(1024 * 1024 * 1024 * 500),
      available_bytes: String(1024 * 1024 * 1024 * 100),
    };
  }
  const commands = await loadNativeCommands();
  return runCommand("queryDiskSpace", () => commands.queryDiskSpace(path));
}

/**
 * Extract the OS-associated file-type icon for a file name. Returns a PNG
 * base64 data URL suitable for `<img src>`. On non-Windows or when extraction
 * fails, `data_url` is `null` so the caller can fall back to a generic icon.
 */
export async function extractSystemFileIcon(fileName: string): Promise<SystemFileIcon> {
  if (!isTauriRuntime()) {
    // Browser preview mock: no system icons available.
    return { data_url: null, mime_hint: null };
  }
  const commands = await loadNativeCommands();
  return runCommand("extractSystemFileIcon", () => commands.extractSystemFileIcon(fileName));
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
  await runCommand("focusMainWindowFromFloating", () => commands.focusMainWindowFromFloating());
}

export async function showTrayMenuAt(logicalX: number, logicalY: number): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock show tray menu at", logicalX, logicalY);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("showTrayMenuAt", () => commands.showTrayMenuAt(logicalX, logicalY));
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
  return runCommand("updateBrowserCaptureSettings", () => commands.updateBrowserCaptureSettings(input));
}

export async function listClassificationRules(): Promise<ClassificationRule[]> {
  const commands = await loadNativeCommands();
  return runCommand("listClassificationRules", () => commands.listClassificationRules());
}

export async function createClassificationRule(input: ClassificationRuleInput): Promise<ClassificationRule> {
  const commands = await loadNativeCommands();
  return runCommand("createClassificationRule", () => commands.createClassificationRule(input));
}

export async function updateClassificationRule(
  id: string,
  input: ClassificationRuleInput,
): Promise<ClassificationRule> {
  const commands = await loadNativeCommands();
  return runCommand("updateClassificationRule", () => commands.updateClassificationRule(id, input));
}

export async function deleteClassificationRule(id: string): Promise<void> {
  const commands = await loadNativeCommands();
  await runCommand("deleteClassificationRule", () => commands.deleteClassificationRule(id));
}

export async function reorderClassificationRules(ids: string[]): Promise<void> {
  const commands = await loadNativeCommands();
  await runCommand("reorderClassificationRules", () => commands.reorderClassificationRules(ids));
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).createTask(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("createTask", () => commands.createTask(input)));
}

export async function updateTaskTransferOptions(input: UpdateTaskTransferOptionsInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).updateTaskTransferOptions(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("updateTaskTransferOptions", () => commands.updateTaskTransferOptions(input)));
}

export async function reorderQueuedTasks(taskIds: string[]): Promise<void> {
  if (!isTauriRuntime()) {
    log.debug("mock reorder queued tasks", taskIds);
    return;
  }
  const commands = await loadNativeCommands();
  await runCommand("reorderQueuedTasks", () => commands.reorderQueuedTasks(taskIds));
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

export async function computeFileHash(id: string, algorithm: ChecksumAlgorithm): Promise<string> {
  if (!isTauriRuntime()) {
    return "mock-hash-value";
  }
  const commands = await loadNativeCommands();
  return runCommand("computeFileHash", () => commands.computeFileHash(id, algorithm));
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

export async function listMetalinkMirrors(id: string): Promise<MetalinkMirrorView[]> {
  if (!isTauriRuntime()) {
    return [];
  }
  const commands = await loadNativeCommands();
  return runCommand("listMetalinkMirrors", () => commands.listMetalinkMirrors(id));
}

export async function retryTaskWithMirror(id: string, mirrorUrl: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).retryTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("retryTaskWithMirror", () => commands.retryTaskWithMirror(id, mirrorUrl)));
}

export async function finishLiveRecording(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).finishLiveRecording(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("finishLiveRecording", () => commands.finishLiveRecording(id)));
}

export async function resolveTaskAttention(input: ResolveTaskAttentionInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).resolveTaskAttention(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("resolveTaskAttention", () => commands.resolveTaskAttention(input)));
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

export async function bulkDeleteTasks(ids: string[], deleteFile = false): Promise<number> {
  if (!isTauriRuntime()) {
    const adapter = await loadBrowserAdapter();
    for (const id of ids) await adapter.deleteTask(id, deleteFile);
    return ids.length;
  }
  const commands = await loadNativeCommands();
  return runCommand("bulkDeleteTasks", () => commands.bulkDeleteTasks(ids, deleteFile));
}

export async function bulkTaskAction(ids: string[], action: "pause" | "resume" | "retry"): Promise<number> {
  if (!isTauriRuntime()) {
    const adapter = await loadBrowserAdapter();
    let succeeded = 0;
    for (const id of ids) {
      try {
        if (action === "pause") await adapter.pauseTask(id);
        else if (action === "resume") await adapter.resumeTask(id);
        else if (action === "retry") await adapter.retryTask(id);
        succeeded += 1;
      } catch {
        // skip individual failures
      }
    }
    return succeeded;
  }
  const commands = await loadNativeCommands();
  return runCommand("bulkTaskAction", () => commands.bulkTaskAction(ids, action));
}

export async function bulkTaskActionGlobal(
  action: "pause" | "resume",
): Promise<{ succeeded: number; skipped: number; failed: number }> {
  if (!isTauriRuntime()) {
    const adapter = await loadBrowserAdapter();
    const tasks = await adapter.listTasks();
    const statuses =
      action === "pause"
        ? new Set(["downloading", "retrying", "queued"])
        : new Set(["paused", "failed", "waiting_network"]);
    const ids = tasks.filter((task) => statuses.has(task.status)).map((task) => task.id);
    let succeeded = 0;
    let failed = 0;
    for (const id of ids) {
      try {
        if (action === "pause") await adapter.pauseTask(id);
        else await adapter.resumeTask(id);
        succeeded += 1;
      } catch {
        failed += 1;
      }
    }
    return { succeeded, skipped: 0, failed };
  }
  const commands = await loadNativeCommands();
  return runCommand("bulkTaskActionGlobal", () => commands.bulkTaskActionGlobal(action));
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

export function onTaskProgress(handler: (payload: TaskProgressPayload) => void): Promise<() => void> {
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

export function onQueueChanged(handler: (payload: QueueChangedPayload | null) => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onQueueChanged(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<QueueChangedPayload>(EVENT_QUEUE_CHANGED, (event) => {
      handler(event.payload);
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

export function onClipboardLinkDetected(handler: (payload: ClipboardLinkDetectedPayload) => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onClipboardLinkDetected(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<ClipboardLinkDetectedPayload>(EVENT_CLIPBOARD_LINK_DETECTED, (event) => {
      handler(event.payload);
    }).then((unlisten) => unlisten),
  );
}

export function onBrowserIntegrationChanged(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onBrowserIntegrationChanged(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_BROWSER_INTEGRATION_CHANGED, () => {
      handler();
    }).then((unlisten) => unlisten),
  );
}

export function onCompletionActionRequested(
  handler: (payload: CompletionActionRequestedPayload) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onCompletionActionRequested(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<CompletionActionRequestedPayload>(EVENT_COMPLETION_ACTION_REQUESTED, (event) => {
      handler(event.payload);
    }).then((unlisten) => unlisten),
  );
}

/**
 * UX-6: Real probe-phase events from Rust engines. The NewDownloadDialog
 * subscribes to this to show stage-aware feedback (connecting, fetching
 * manifest, parsing, etc.) instead of the old URL-regex static guess.
 */
export function onProbePhase(handler: (payload: ProbePhasePayload) => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onProbePhase(handler));
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen<ProbePhasePayload>(EVENT_PROBE_PHASE, (event) => {
      handler(event.payload);
    }).then((unlisten) => unlisten),
  );
}

/**
 * Emitted by the Rust backend when the app begins its graceful shutdown
 * (window close or tray Quit). The frontend uses this to show a "saving
 * progress" overlay while active downloads are cancelled and checkpoints
 * are flushed.
 */
export function onShuttingDown(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return Promise.resolve(() => {});
  }
  return import("@tauri-apps/api/event").then(({ listen }) =>
    listen(EVENT_SHUTTING_DOWN, () => handler()).then((unlisten) => unlisten),
  );
}

/**
 * Drag-and-drop file events from the native window. Tauri intercepts native
 * file drops (dragDropEnabled defaults to true), so HTML5 drop events do not
 * fire — we subscribe via the webview API instead.
 *
 * - `onDrop` receives the absolute file paths of dropped files.
 * - `onDragStateChange` tracks the drag-enter/leave overlay state. `paths`
 *   is only provided on enter so callers can pre-filter by extension.
 */
export type FileDropDragState = { active: boolean; paths?: string[] };

export function onFileDrop(
  onDrop: (paths: string[]) => void,
  onDragStateChange?: (state: FileDropDragState) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return import("@/lib/tauri-browser").then((adapter) => adapter.onFileDrop(onDrop, onDragStateChange));
  }
  return import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
    getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as {
        type: "enter" | "over" | "drop" | "leave";
        paths: string[];
      };
      if (payload.type === "drop") {
        onDrop(payload.paths ?? []);
        onDragStateChange?.({ active: false });
        return;
      }
      if (payload.type === "enter") {
        onDragStateChange?.({ active: true, paths: payload.paths });
        return;
      }
      if (payload.type === "leave") {
        onDragStateChange?.({ active: false });
      }
    }),
  );
}

export { isTauriRuntime };

export async function getAppVersion(): Promise<string> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getAppVersion();
  }
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}
