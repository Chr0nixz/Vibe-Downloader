// Task store facade — re-exports from split stores for convenience.

export type { SpeedHistoryByTaskId, SpeedSample } from "./speed-history-store";
export { useSpeedHistoryStore } from "./speed-history-store";
export type {
  FileTypeFilter,
  NavFilter,
  ResumeFilter,
  TaskFilters,
  TaskSortDirection,
  TaskSortKey,
  TaskStats,
} from "./task-data-store";
export {
  calculateTaskStats,
  EMPTY_TASK_STATS,
  indexTasks,
  mapTasksById,
  normalizeTaskStatsSnapshot,
  taskCursorInput,
  taskPageInput,
  useTaskDataStore,
} from "./task-data-store";
export {
  failureKind,
  filterTasks,
  mergeTasksFromServer,
  taskFileType,
} from "./task-query";
export { useTaskUIStore } from "./task-ui-store";
