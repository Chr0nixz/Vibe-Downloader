import { create } from "zustand";

export type ToastTone = "success" | "error" | "info";

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface AppToast {
  id: string;
  tone: ToastTone;
  title: string;
  description?: string;
  action?: ToastAction;
  /** Optional business key for deduplication. When set, addToast will update
   * an existing toast with the same key instead of creating a new one. */
  key?: string;
}

interface ToastStore {
  toasts: AppToast[];
  addToast: (toast: Omit<AppToast, "id">) => string;
  updateToast: (id: string, patch: Partial<Pick<AppToast, "title" | "description" | "tone">>) => void;
  dismissToast: (id: string) => void;
  clearToasts: () => void;
}

let toastSequence = 0;

export const useToastStore = create<ToastStore>((set, get) => ({
  toasts: [],
  addToast: (toast) => {
    // If a toast with the same business key already exists, update it instead
    // of creating a duplicate. This prevents toast spam during bulk operations.
    if (toast.key) {
      const existing = get().toasts.find((t) => t.key === toast.key);
      if (existing) {
        set((state) => ({
          toasts: state.toasts.map((t) => (t.id === existing.id ? { ...t, ...toast } : t)),
        }));
        return existing.id;
      }
    }
    const id = `toast-${Date.now()}-${toastSequence++}`;
    set((state) => ({
      toasts: [{ ...toast, id }, ...state.toasts].slice(0, 4),
    }));
    return id;
  },
  updateToast: (id, patch) =>
    set((state) => ({
      toasts: state.toasts.map((toast) => (toast.id === id ? { ...toast, ...patch } : toast)),
    })),
  dismissToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),
  clearToasts: () => set({ toasts: [] }),
}));
