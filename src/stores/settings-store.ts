import { create } from "zustand";

import type { AppSettings } from "@/generated/bindings";

interface SettingsStore {
  settings: AppSettings | null;
  loading: boolean;
  error: string | null;
  setSettings: (settings: AppSettings) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useSettingsStore = create<SettingsStore>((set) => ({
  settings: null,
  loading: true,
  error: null,
  setSettings: (settings) => set({ settings }),
  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
}));
