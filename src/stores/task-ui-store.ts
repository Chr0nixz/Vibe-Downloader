import { create } from "zustand";

import type { NavFilter, TaskFilters, TaskSortDirection, TaskSortKey } from "./task-data-store";

/* ── Store interface ── */

interface TaskUIStore {
  selectedId: string | null;
  selectedIds: string[];
  selectionAnchorId: string | null;
  nav: NavFilter;
  search: string;
  sortKey: TaskSortKey;
  sortDirection: TaskSortDirection;
  filters: TaskFilters;
  detailOpen: boolean;
  /** Tasks hidden from the list while a soft-delete undo toast is active.
   * Cleared on commit (hard delete) or undo (restore). */
  pendingDeleteIds: string[];
  selectTask: (id: string | null) => void;
  toggleTaskSelected: (id: string) => void;
  setTaskSelected: (id: string, selected: boolean) => void;
  setSelectedIds: (ids: string[]) => void;
  clearSelectedIds: () => void;
  setSelectionAnchor: (id: string | null) => void;
  setNav: (nav: NavFilter) => void;
  setSearch: (search: string) => void;
  setSort: (key: TaskSortKey, direction?: TaskSortDirection) => void;
  setFilters: (filters: Partial<TaskFilters>) => void;
  setDetailOpen: (open: boolean) => void;
  addPendingDelete: (id: string) => void;
  addPendingDeletes: (ids: string[]) => void;
  removePendingDelete: (id: string) => void;
  clearPendingDeletes: () => void;
}

/* ── Store ── */

export const useTaskUIStore = create<TaskUIStore>((set) => ({
  selectedId: null,
  selectedIds: [],
  selectionAnchorId: null,
  nav: "all",
  search: "",
  sortKey: "updated_at",
  sortDirection: "desc",
  filters: {
    fileType: "all",
    source: "all",
    failure: "all",
    resume: "all",
  },
  detailOpen: false,
  pendingDeleteIds: [],

  selectTask: (id) => set({ selectedId: id, selectionAnchorId: id }),

  toggleTaskSelected: (id) =>
    set((state) => ({
      selectedIds: state.selectedIds.includes(id)
        ? state.selectedIds.filter((taskId) => taskId !== id)
        : [...state.selectedIds, id],
    })),

  setTaskSelected: (id, selected) =>
    set((state) => ({
      selectedIds: selected
        ? Array.from(new Set([...state.selectedIds, id]))
        : state.selectedIds.filter((taskId) => taskId !== id),
    })),

  setSelectedIds: (ids) => set({ selectedIds: Array.from(new Set(ids)) }),

  clearSelectedIds: () => set({ selectedIds: [], selectionAnchorId: null }),

  setSelectionAnchor: (id) => set({ selectionAnchorId: id }),

  setNav: (nav) => set({ nav }),

  setSearch: (search) => set({ search }),

  setSort: (key, direction) =>
    set((state) => ({
      sortKey: key,
      sortDirection: direction ?? (state.sortKey === key && state.sortDirection === "desc" ? "asc" : "desc"),
    })),

  setFilters: (filters) => set((state) => ({ filters: { ...state.filters, ...filters } })),

  setDetailOpen: (open) => set({ detailOpen: open }),

  addPendingDelete: (id) =>
    set((state) =>
      state.pendingDeleteIds.includes(id) ? state : { pendingDeleteIds: [...state.pendingDeleteIds, id] },
    ),

  addPendingDeletes: (ids) =>
    set((state) => {
      const existing = new Set(state.pendingDeleteIds);
      const next = [...state.pendingDeleteIds];
      for (const id of ids) {
        if (!existing.has(id)) {
          next.push(id);
          existing.add(id);
        }
      }
      return next.length === state.pendingDeleteIds.length ? state : { pendingDeleteIds: next };
    }),

  removePendingDelete: (id) =>
    set((state) => ({
      pendingDeleteIds: state.pendingDeleteIds.filter((taskId) => taskId !== id),
    })),

  clearPendingDeletes: () => set({ pendingDeleteIds: [] }),
}));
