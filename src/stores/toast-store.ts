import { create } from "zustand";

import { isModalFocusActive, subscribeModalFocus } from "@/lib/modal-focus";

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

/** UX-19: hard cap on the toast stack (and the deferred queue). */
const TOAST_STACK_LIMIT = 20;

/**
 * UX-19: trim the stack to the cap, evicting the oldest entries. An evicted
 * toast still leaves through its onAutoCommit — silently dropping a
 * soft-delete toast would strand the task in pendingDeleteIds forever
 * (hidden from the list and undeletable).
 */
function capToasts(list: AppToast[]): AppToast[] {
  if (list.length <= TOAST_STACK_LIMIT) {
    return list;
  }
  for (const evicted of list.slice(TOAST_STACK_LIMIT)) {
    evicted.onAutoCommit?.();
  }
  return list.slice(0, TOAST_STACK_LIMIT);
}

/**
 * Toasts created while a modal owned focus. They are kept out of `toasts` so the
 * viewport never renders them behind a scrim, where the user could not read them
 * and the auto-dismiss timer would run out before the dialog closed. Flushed when
 * the last modal unmounts, so the confirmation still arrives — just later.
 */
let deferredToasts: AppToast[] = [];

function flushDeferredToasts() {
  if (deferredToasts.length === 0) return;
  const ready = deferredToasts;
  deferredToasts = [];
  const current = useToastStore.getState().toasts;
  useToastStore.setState({ toasts: capToasts([...ready.reverse(), ...current]) });
}

subscribeModalFocus(() => {
  if (!isModalFocusActive()) flushDeferredToasts();
});

export const useToastStore = create<ToastStore>((set, get) => ({
  toasts: [],
  addToast: (toast) => {
    // If a toast with the same business key already exists, update it instead
    // of creating a duplicate. This prevents toast spam during bulk operations.
    if (toast.key) {
      const existing = get().toasts.find((t) => t.key === toast.key) ?? deferredToasts.find((t) => t.key === toast.key);
      if (existing) {
        // Replacing a soft-delete toast must still commit the previous action
        // so pending deletes are not left without a commit path.
        if (existing.onAutoCommit && existing.onAutoCommit !== toast.onAutoCommit) {
          existing.onAutoCommit();
        }
        const patchIn = (list: AppToast[]) => list.map((t) => (t.id === existing.id ? { ...t, ...toast } : t));
        if (deferredToasts.some((t) => t.id === existing.id)) {
          deferredToasts = patchIn(deferredToasts);
        } else {
          set((state) => ({ toasts: patchIn(state.toasts) }));
        }
        return existing.id;
      }
    }
    const id = `toast-${Date.now()}-${toastSequence++}`;
    const created: AppToast = { ...toast, id };
    // Errors are not deferred: they may explain why a control inside the open
    // dialog is not responding, so hiding them would strand the user.
    if (created.tone !== "error" && isModalFocusActive()) {
      deferredToasts = capToasts([created, ...deferredToasts]);
      return id;
    }
    set({ toasts: capToasts([created, ...get().toasts]) });
    return id;
  },
  updateToast: (id, patch) => {
    deferredToasts = deferredToasts.map((toast) => (toast.id === id ? { ...toast, ...patch } : toast));
    set((state) => ({
      toasts: state.toasts.map((toast) => (toast.id === id ? { ...toast, ...patch } : toast)),
    }));
  },
  dismissToast: (id) => {
    deferredToasts = deferredToasts.filter((toast) => toast.id !== id);
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    }));
  },
  clearToasts: () => {
    // Dismissing the stack accepts pending soft-deletes (same as X / timeout).
    for (const toast of get().toasts) {
      toast.onAutoCommit?.();
    }
    for (const toast of deferredToasts) {
      toast.onAutoCommit?.();
    }
    deferredToasts = [];
    set({ toasts: [] });
  },
}));
