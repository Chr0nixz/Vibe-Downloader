import { create } from "zustand";

export type ToastTone = "success" | "error" | "info";

/** Default auto-dismiss window for informational toasts (ms). */
export const TOAST_TIMEOUT_MS = 4800;
/** Extended window for undo toasts so the Undo action stays reachable (ms). */
export const UNDO_TOAST_TIMEOUT_MS = 7000;

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
  /** Optional per-toast duration in ms. When omitted, the default
   * `TOAST_TIMEOUT_MS` is used. Undo toasts should set this so the Undo
   * action stays reachable. */
  durationMs?: number;
  /**
   * Called when the toast leaves without Undo (timeout, dismiss X, or clear).
   * Soft-delete commits hard delete here so hover-paused toast timers also
   * delay the commit — one lifecycle, not a separate setTimeout.
   */
  onAutoCommit?: () => void;
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
        // Replacing a soft-delete toast must still commit the previous action
        // so pending deletes are not left without a commit path.
        if (existing.onAutoCommit && existing.onAutoCommit !== toast.onAutoCommit) {
          existing.onAutoCommit();
        }
        set((state) => ({
          toasts: state.toasts.map((t) => (t.id === existing.id ? { ...t, ...toast } : t)),
        }));
        return existing.id;
      }
    }
    const id = `toast-${Date.now()}-${toastSequence++}`;
    set((state) => ({
      toasts: [{ ...toast, id }, ...state.toasts].slice(0, 20),
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
  clearToasts: () => {
    // Dismissing the stack accepts pending soft-deletes (same as X / timeout).
    for (const toast of get().toasts) {
      toast.onAutoCommit?.();
    }
    set({ toasts: [] });
  },
}));
