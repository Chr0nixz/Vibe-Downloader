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
}));
