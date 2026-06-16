// Task store facade — re-exports from split stores for convenience.

export {
  useTaskDataStore,
  taskPageInput,
  taskCursorInput,
  normalizeTaskStatsSnapshot,
  EMPTY_TASK_STATS,
  calculateTaskStats,
  indexTasks,
  mapTasksById,
} from "./task-data-store";

export type {
  TaskStats,
  TaskFilters,
  NavFilter,
  TaskSortKey,
  TaskSortDirection,
  FileTypeFilter,
  ResumeFilter,
} from "./task-data-store";

export { useTaskUIStore } from "./task-ui-store";

export {
  failureKind,
  filterTasks,
  mergeTasksFromServer,
  taskFileType,
} from "./task-query";

export { useSpeedHistoryStore } from "./speed-history-store";
export type { SpeedSample, SpeedHistoryByTaskId } from "./speed-history-store";
