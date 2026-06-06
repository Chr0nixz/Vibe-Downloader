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
}

interface ToastStore {
  toasts: AppToast[];
  addToast: (toast: Omit<AppToast, "id">) => string;
  dismissToast: (id: string) => void;
  clearToasts: () => void;
}

let toastSequence = 0;

export const useToastStore = create<ToastStore>((set) => ({
  toasts: [],
  addToast: (toast) => {
    const id = `toast-${Date.now()}-${toastSequence++}`;
    set((state) => ({
      toasts: [{ ...toast, id }, ...state.toasts].slice(0, 4),
    }));
    return id;
  },
  dismissToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),
  clearToasts: () => set({ toasts: [] }),
}));
