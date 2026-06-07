import { isTauriRuntime } from "@/lib/runtime";
import { createLogger } from "@/lib/logger";
import type {
  AppSettings,
  BrowserIntegrationStatus,
  BrowserIntegrationUpdateInput,
  CreateTaskInput,
  ProbeTaskInput,
  ProbeTaskPayload,
  UpdateSettingsInput,
} from "@/generated/bindings";
import type { Task } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";
import type { TaskProgressPayload } from "@/types/task-progress";

export const EVENT_TASK_PROGRESS = "task.progress";
export const EVENT_QUEUE_CHANGED = "queue.changed";
export const EVENT_SETTINGS_CHANGED = "settings.changed";
export const EVENT_BROWSER_INTEGRATION_CHANGED = "browser.integration.changed";

const log = createLogger("tauri");

type CommandResult<T, E> =
  | { status: "ok"; data: T }
  | { status: "error"; error: E };

function unwrapCommand<T, E>(result: CommandResult<T, E>): T {
  if (result.status === "ok") return result.data;
  throw result.error;
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

export async function listTaskSegments(taskId: string): Promise<TaskSegment[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTaskSegments(taskId);
  }
  const commands = await loadNativeCommands();
  const segments = await runCommand("listTaskSegments", () => commands.listTaskSegments(taskId));
  return segments.map(normalizeTaskSegment);
}

export async function seedMockTasks(): Promise<Task[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).seedMockTasks();
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

export async function createTask(input: CreateTaskInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).createTask(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(await runCommand("createTask", () => commands.createTask(input)));
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
