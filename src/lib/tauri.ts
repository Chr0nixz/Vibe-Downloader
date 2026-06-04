import { listen } from "@tauri-apps/api/event";

import { commands } from "@/generated/bindings";
import type { CreateTaskInput, ProbeTaskInput, ProbeTaskPayload } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { normalizeTask } from "@/types/task";
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

export async function listTasks(): Promise<Task[]> {
  const tasks = unwrapCommand(await commands.listTasks());
  return tasks.map(normalizeTask);
}

export async function getTask(id: string): Promise<Task | null> {
  const task = unwrapCommand(await commands.getTask(id));
  return task ? normalizeTask(task) : null;
}

export async function seedMockTasks(): Promise<Task[]> {
  const tasks = unwrapCommand(await commands.seedMockTasks());
  return tasks.map(normalizeTask);
}

export async function createTask(input: CreateTaskInput): Promise<Task> {
  return normalizeTask(unwrapCommand(await commands.createTask(input)));
}

export async function probeTask(input: ProbeTaskInput): Promise<ProbeTaskPayload> {
  return unwrapCommand(await commands.probeTask(input));
}

export async function pauseTask(id: string): Promise<Task> {
  return normalizeTask(unwrapCommand(await commands.pauseTask(id)));
}

export async function resumeTask(id: string): Promise<Task> {
  return normalizeTask(unwrapCommand(await commands.resumeTask(id)));
}

export async function retryTask(id: string): Promise<Task> {
  return normalizeTask(unwrapCommand(await commands.retryTask(id)));
}

export async function cancelTask(id: string): Promise<Task> {
  return normalizeTask(unwrapCommand(await commands.cancelTask(id)));
}

export async function deleteTask(id: string, deleteFile = false): Promise<void> {
  unwrapCommand(await commands.deleteTask(id, deleteFile));
}

export async function openTaskFile(id: string): Promise<void> {
  unwrapCommand(await commands.openTaskFile(id));
}

export async function openTaskFolder(id: string): Promise<void> {
  unwrapCommand(await commands.openTaskFolder(id));
}

export function onTaskProgress(
  handler: (payload: TaskProgressPayload) => void,
): Promise<() => void> {
  return listen<TaskProgressPayload>(EVENT_TASK_PROGRESS, (event) => {
    handler(event.payload);
  }).then((unlisten) => unlisten);
}

export function onQueueChanged(handler: () => void): Promise<() => void> {
  return listen(EVENT_QUEUE_CHANGED, () => {
    handler();
  }).then((unlisten) => unlisten);
}
