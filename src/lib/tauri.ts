import { isTauriRuntime } from "@/lib/runtime";
import type { CreateTaskInput, ProbeTaskInput, ProbeTaskPayload } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { normalizeTaskSegment } from "@/types/task-segment";
import type { TaskProgressPayload } from "@/types/task-progress";

export const EVENT_TASK_PROGRESS = "task.progress";
export const EVENT_QUEUE_CHANGED = "queue.changed";

type CommandResult<T, E> =
  | { status: "ok"; data: T }
  | { status: "error"; error: E };

function unwrapCommand<T, E>(result: CommandResult<T, E>): T {
  if (result.status === "ok") return result.data;
  throw result.error;
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
  const tasks = unwrapCommand(await commands.listTasks());
  return tasks.map(normalizeTask);
}

export async function getTask(id: string): Promise<Task | null> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).getTask(id);
  }
  const commands = await loadNativeCommands();
  const task = unwrapCommand(await commands.getTask(id));
  return task ? normalizeTask(task) : null;
}

export async function listTaskSegments(taskId: string): Promise<TaskSegment[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).listTaskSegments(taskId);
  }
  const commands = await loadNativeCommands();
  const segments = unwrapCommand(await commands.listTaskSegments(taskId));
  return segments.map(normalizeTaskSegment);
}

export async function seedMockTasks(): Promise<Task[]> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).seedMockTasks();
  }
  const commands = await loadNativeCommands();
  const tasks = unwrapCommand(await commands.seedMockTasks());
  return tasks.map(normalizeTask);
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).createTask(input);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(unwrapCommand(await commands.createTask(input)));
}

export async function probeTask(input: ProbeTaskInput): Promise<ProbeTaskPayload> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).probeTask(input);
  }
  const commands = await loadNativeCommands();
  return unwrapCommand(await commands.probeTask(input));
}

export async function pauseTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).pauseTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(unwrapCommand(await commands.pauseTask(id)));
}

export async function resumeTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).resumeTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(unwrapCommand(await commands.resumeTask(id)));
}

export async function retryTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).retryTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(unwrapCommand(await commands.retryTask(id)));
}

export async function cancelTask(id: string): Promise<Task> {
  if (!isTauriRuntime()) {
    return (await loadBrowserAdapter()).cancelTask(id);
  }
  const commands = await loadNativeCommands();
  return normalizeTask(unwrapCommand(await commands.cancelTask(id)));
}

export async function deleteTask(id: string, deleteFile = false): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).deleteTask(id, deleteFile);
    return;
  }
  const commands = await loadNativeCommands();
  unwrapCommand(await commands.deleteTask(id, deleteFile));
}

export async function openTaskFile(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).openTaskFile(id);
    return;
  }
  const commands = await loadNativeCommands();
  unwrapCommand(await commands.openTaskFile(id));
}

export async function openTaskFolder(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    await (await loadBrowserAdapter()).openTaskFolder(id);
    return;
  }
  const commands = await loadNativeCommands();
  unwrapCommand(await commands.openTaskFolder(id));
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

export { isTauriRuntime };
