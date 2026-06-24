import { create } from "zustand";

export interface SpeedSample {
  at: number;
  speedBps: number;
}

export type SpeedHistoryByTaskId = Record<string, SpeedSample[]>;

const SPEED_HISTORY_LIMIT = 60;

interface SpeedHistoryState {
  history: SpeedHistoryByTaskId;
  appendSample: (taskId: string, sample: SpeedSample) => void;
  appendBatch: (entries: Array<{ taskId: string; sample: SpeedSample }>) => void;
  pruneToIds: (activeIds: Set<string>) => void;
  clearTask: (taskId: string) => void;
  clearAll: () => void;
}

export const useSpeedHistoryStore = create<SpeedHistoryState>((set, get) => ({
  history: {},

  appendSample: (taskId, sample) => {
    const current = get().history;
    const existing = current[taskId] ?? [];
    const updated = existing.length >= SPEED_HISTORY_LIMIT ? [...existing.slice(1), sample] : [...existing, sample];
    // Only update the specific task's array, not the entire map
    set({ history: { ...current, [taskId]: updated } });
  },

  appendBatch: (entries) => {
    if (entries.length === 0) return;
    const current = get().history;
    // Shallow-copy the map (Zustand requires a new reference to trigger updates),
    // but only create new arrays for tasks that actually have new samples.
    // Tasks without samples keep their original array reference (structural sharing).
    const next = { ...current };
    for (const { taskId, sample } of entries) {
      const existing = next[taskId] ?? [];
      next[taskId] = existing.length >= SPEED_HISTORY_LIMIT ? [...existing.slice(1), sample] : [...existing, sample];
    }
    set({ history: next });
  },

  pruneToIds: (activeIds) => {
    const current = get().history;
    let changed = false;
    for (const id of Object.keys(current)) {
      if (!activeIds.has(id)) {
        changed = true;
        break;
      }
    }
    if (!changed) return;
    const next: SpeedHistoryByTaskId = {};
    for (const id of activeIds) {
      if (current[id]) next[id] = current[id];
    }
    set({ history: next });
  },

  clearTask: (taskId) => {
    const current = get().history;
    if (!(taskId in current)) return;
    const { [taskId]: _, ...rest } = current;
    set({ history: rest });
  },

  clearAll: () => set({ history: {} }),
}));
