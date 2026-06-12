export interface SpeedSample {
  at: number;
  speedBps: number;
}

export type SpeedHistoryByTaskId = Record<string, SpeedSample[]>;

const SPEED_HISTORY_LIMIT = 60;

export function pruneSpeedHistory(
  history: SpeedHistoryByTaskId,
  activeTaskIds: Set<string>,
): SpeedHistoryByTaskId {
  return Object.fromEntries(
    Object.entries(history).filter(([id]) => activeTaskIds.has(id)),
  );
}

export function appendSpeedSample(
  history: SpeedHistoryByTaskId,
  taskId: string,
  sample: SpeedSample,
): SpeedHistoryByTaskId {
  return {
    ...history,
    [taskId]: [...(history[taskId] ?? []), sample].slice(-SPEED_HISTORY_LIMIT),
  };
}
