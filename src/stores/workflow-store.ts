import { create } from "zustand";
import type { WorkflowEntry } from "@/generated/bindings";
import { deleteWorkflowEntry, listWorkflowLibrary, saveWorkflowEntry } from "@/lib/workflow-library";

let epoch = 0;
export const useWorkflowStore = create<{
  entries: WorkflowEntry[];
  unsupportedCount: number;
  loading: boolean;
  error: boolean;
  load: () => Promise<void>;
  save: (entry: WorkflowEntry) => Promise<void>;
  remove: (id: string) => Promise<void>;
}>((set, get) => ({
  entries: [],
  unsupportedCount: 0,
  loading: false,
  error: false,
  load: async () => {
    if (get().loading) return;
    const current = ++epoch;
    set({ loading: true, error: false });
    try {
      const library = await listWorkflowLibrary();
      if (current === epoch) set({ ...library });
    } catch {
      if (current === epoch) set({ error: true });
    } finally {
      if (current === epoch) set({ loading: false });
    }
  },
  save: async (entry) => {
    await saveWorkflowEntry(entry);
    ++epoch;
    set((state) => ({
      entries: [...state.entries.filter((row) => row.id !== entry.id), entry],
      loading: false,
      error: false,
    }));
  },
  remove: async (id) => {
    await deleteWorkflowEntry(id);
    ++epoch;
    set((state) => ({ entries: state.entries.filter((row) => row.id !== id), loading: false, error: false }));
  },
}));
