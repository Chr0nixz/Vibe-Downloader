import i18n from "i18next";
import { initReactI18next } from "react-i18next";

const en = {
  app: {
    name: "Vibe Downloader",
    navAria: "Main navigation",
  },
  nav: {
    all: "All tasks",
    downloading: "Downloading",
    paused: "Paused",
    completed: "Completed",
    failed: "Failed",
    settings: "Settings",
  },
  task: {
    status: {
      downloading: "Downloading",
      paused: "Paused",
      completed: "Completed",
      failed: "Failed",
      retrying: "Retrying",
      waiting_network: "Waiting for network",
      needs_attention: "Needs attention",
      queued: "Queued",
    },
    eta: "ETA",
    connections: "{{count}} connections",
    progressAria: "{{name}} download progress, {{percent}}",
  },
  segment: {
    status: {
      pending: "Pending",
      downloading: "Downloading",
      completed: "Completed",
      failed: "Failed",
    },
  },
  commandBar: {
    newDownload: "New download",
    newDownloadAria: "New download",
    start: "Start",
    pause: "Pause",
    delete: "Delete",
    speedLimit: "Speed limit",
    searchPlaceholder: "Search tasks",
    searchAria: "Search tasks",
    palette: "Command palette",
  },
  titleBar: {
    minimize: "Minimize",
    maximize: "Maximize",
    close: "Close",
  },
  statusBar: {
    total: "Total",
    active: "Active",
    queued: "Queued",
    noGlobalSpeedLimit: "No global speed limit",
  },
  taskList: {
    loading: "Loading tasks\u2026",
    empty: "No tasks in this view.",
    aria: "Download tasks",
  },
  settings: {
    title: "Settings",
    placeholder: "Default download directory, speed limits, and browser integration will ship in the next milestone.",
  },
  deleteDialog: {
    title: "Delete task",
    messageWithName: "Delete \"{{name}}\" from the task list?",
    messageGeneric: "Delete this task from the task list?",
    cancel: "Cancel",
    deleteRecord: "Delete record",
    deleteFilesToo: "Delete files too",
  },
  newDownload: {
    title: "New download",
    url: "URL",
    urlPlaceholder: "https://example.com/file.zip",
    detect: "Detect",
    detecting: "Detecting\u2026",
    saveDir: "Save directory",
    saveDirPlaceholder: "Default Downloads folder",
    fileName: "File name",
    fileNamePlaceholder: "Detected from the server",
    probeFile: "File",
    probeSize: "Size",
    probeHost: "Host",
    probeResume: "Resume",
    resumeSupported: "Supported",
    resumeUnavailable: "Unavailable",
    cancel: "Cancel",
    start: "Start download",
    starting: "Starting\u2026",
  },
  palette: {
    title: "Command palette",
    resetMockTasks: "Reset mock tasks",
  },
  taskDetails: {
    overview: "Overview",
    chunks: "Chunks",
    connections: "Connections",
    progress: "Progress",
    speed: "Speed",
    eta: "ETA",
    noChunks: "No chunk data for this task.",
    chunkRange: "Range",
    chunkProgress: "Progress",
    chunkRetries: "Retries",
    chunkProgressAria: "Chunk {{range}} progress, {{percent}}",
    chunksPlaceholder: "Chunk heatmap placeholder for HTTP MVP.",
    connectionsPlaceholder: "Connections tab placeholder.",
    close: "Close details",
    closeBackdrop: "Close details",
    drawerAria: "Task details",
    drawerDescription: "Details for {{name}}",
  },
  actions: {
    resume: "Resume",
    pause: "Pause",
    retry: "Retry",
    openFile: "Open file",
    openFolder: "Open folder",
    cancel: "Cancel",
  },
  locale: {
    label: "Language",
    en: "English",
    zhCN: "Chinese (Simplified)",
  },
} as const;

const zhCN = {
  app: {
    name: "Vibe Downloader",
    navAria: "\u4e3b\u5bfc\u822a",
  },
  nav: {
    all: "\u5168\u90e8\u4efb\u52a1",
    downloading: "\u4e0b\u8f7d\u4e2d",
    paused: "\u5df2\u6682\u505c",
    completed: "\u5df2\u5b8c\u6210",
    failed: "\u5931\u8d25",
    settings: "\u8bbe\u7f6e",
  },
  task: {
    status: {
      downloading: "\u4e0b\u8f7d\u4e2d",
      paused: "\u5df2\u6682\u505c",
      completed: "\u5df2\u5b8c\u6210",
      failed: "\u5931\u8d25",
      retrying: "\u91cd\u8bd5\u4e2d",
      waiting_network: "\u7b49\u5f85\u7f51\u7edc",
      needs_attention: "\u9700\u8981\u5904\u7406",
      queued: "\u6392\u961f\u4e2d",
    },
    eta: "\u9884\u8ba1\u5269\u4f59",
    connections: "{{count}} \u4e2a\u8fde\u63a5",
    progressAria: "{{name}} \u4e0b\u8f7d\u8fdb\u5ea6\uff0c{{percent}}",
  },
  segment: {
    status: {
      pending: "\u7b49\u5f85",
      downloading: "\u4e0b\u8f7d\u4e2d",
      completed: "\u5df2\u5b8c\u6210",
      failed: "\u5931\u8d25",
    },
  },
  commandBar: {
    newDownload: "\u65b0\u5efa\u4e0b\u8f7d",
    newDownloadAria: "\u65b0\u5efa\u4e0b\u8f7d",
    start: "\u5f00\u59cb",
    pause: "\u6682\u505c",
    delete: "\u5220\u9664",
    speedLimit: "\u901f\u5ea6\u9650\u5236",
    searchPlaceholder: "\u641c\u7d22\u4efb\u52a1",
    searchAria: "\u641c\u7d22\u4efb\u52a1",
    palette: "\u547d\u4ee4\u9762\u677f",
  },
  titleBar: {
    minimize: "\u6700\u5c0f\u5316",
    maximize: "\u6700\u5927\u5316",
    close: "\u5173\u95ed",
  },
  statusBar: {
    total: "\u603b\u8ba1",
    active: "\u6d3b\u8dc3",
    queued: "\u6392\u961f",
    noGlobalSpeedLimit: "\u672a\u8bbe\u7f6e\u5168\u5c40\u901f\u5ea6\u9650\u5236",
  },
  taskList: {
    loading: "\u6b63\u5728\u52a0\u8f7d\u4efb\u52a1\u2026",
    empty: "\u6b64\u89c6\u56fe\u4e2d\u6ca1\u6709\u4efb\u52a1\u3002",
    aria: "\u4e0b\u8f7d\u4efb\u52a1\u5217\u8868",
  },
  settings: {
    title: "\u8bbe\u7f6e",
    placeholder: "\u9ed8\u8ba4\u4e0b\u8f7d\u76ee\u5f55\u3001\u901f\u5ea6\u9650\u5236\u4e0e\u6d4f\u89c8\u5668\u96c6\u6210\u5c06\u5728\u4e0b\u4e00\u4e2a\u91cc\u7a0b\u7891\u4e2d\u63d0\u4f9b\u3002",
  },
  deleteDialog: {
    title: "\u5220\u9664\u4efb\u52a1",
    messageWithName: "\u4ece\u4efb\u52a1\u5217\u8868\u4e2d\u5220\u9664\u201c{{name}}\u201d\uff1f",
    messageGeneric: "\u4ece\u4efb\u52a1\u5217\u8868\u4e2d\u5220\u9664\u6b64\u4efb\u52a1\uff1f",
    cancel: "\u53d6\u6d88",
    deleteRecord: "\u4ec5\u5220\u9664\u8bb0\u5f55",
    deleteFilesToo: "\u540c\u65f6\u5220\u9664\u6587\u4ef6",
  },
  newDownload: {
    title: "\u65b0\u5efa\u4e0b\u8f7d",
    url: "URL",
    urlPlaceholder: "https://example.com/file.zip",
    detect: "\u68c0\u6d4b",
    detecting: "\u68c0\u6d4b\u4e2d\u2026",
    saveDir: "\u4fdd\u5b58\u76ee\u5f55",
    saveDirPlaceholder: "\u9ed8\u8ba4\u4e0b\u8f7d\u6587\u4ef6\u5939",
    fileName: "\u6587\u4ef6\u540d",
    fileNamePlaceholder: "\u7531\u670d\u52a1\u5668\u68c0\u6d4b",
    probeFile: "\u6587\u4ef6",
    probeSize: "\u5927\u5c0f",
    probeHost: "\u4e3b\u673a",
    probeResume: "\u65ad\u70b9\u7eed\u4f20",
    resumeSupported: "\u652f\u6301",
    resumeUnavailable: "\u4e0d\u53ef\u7528",
    cancel: "\u53d6\u6d88",
    start: "\u5f00\u59cb\u4e0b\u8f7d",
    starting: "\u6b63\u5728\u542f\u52a8\u2026",
  },
  palette: {
    title: "\u547d\u4ee4\u9762\u677f",
    resetMockTasks: "\u91cd\u7f6e\u6a21\u62df\u4efb\u52a1",
  },
  taskDetails: {
    overview: "\u6982\u89c8",
    chunks: "\u5206\u5757",
    connections: "\u8fde\u63a5",
    progress: "\u8fdb\u5ea6",
    speed: "\u901f\u5ea6",
    eta: "\u9884\u8ba1\u5269\u4f59",
    noChunks: "\u6b64\u4efb\u52a1\u6682\u65e0\u5206\u5757\u6570\u636e\u3002",
    chunkRange: "\u8303\u56f4",
    chunkProgress: "\u8fdb\u5ea6",
    chunkRetries: "\u91cd\u8bd5",
    chunkProgressAria: "\u5206\u5757 {{range}} \u8fdb\u5ea6\uff0c{{percent}}",
    chunksPlaceholder: "HTTP MVP \u5206\u5757\u70ed\u529b\u56fe\u5360\u4f4d\u5185\u5bb9\u3002",
    connectionsPlaceholder: "\u8fde\u63a5\u6807\u7b7e\u5360\u4f4d\u5185\u5bb9\u3002",
    close: "\u5173\u95ed\u8be6\u60c5",
    closeBackdrop: "\u5173\u95ed\u8be6\u60c5",
    drawerAria: "\u4efb\u52a1\u8be6\u60c5",
    drawerDescription: "{{name}} \u7684\u8be6\u60c5",
  },
  actions: {
    resume: "\u7ee7\u7eed",
    pause: "\u6682\u505c",
    retry: "\u91cd\u8bd5",
    openFile: "\u6253\u5f00\u6587\u4ef6",
    openFolder: "\u6253\u5f00\u6587\u4ef6\u5939",
    cancel: "\u53d6\u6d88",
  },
  locale: {
    label: "\u8bed\u8a00",
    en: "English",
    zhCN: "\u7b80\u4f53\u4e2d\u6587",
  },
} as const;

export const LOCALE_STORAGE_KEY = "vibe-locale";

export const SUPPORTED_LOCALES = ["en", "zh-CN"] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

function normalizeLocale(value: string | null | undefined): Locale {
  if (!value) return "en";
  if (value === "zh" || value.startsWith("zh-")) return "zh-CN";
  if (SUPPORTED_LOCALES.includes(value as Locale)) return value as Locale;
  return "en";
}

function detectInitialLocale(): Locale {
  const stored = localStorage.getItem(LOCALE_STORAGE_KEY);
  if (stored) return normalizeLocale(stored);
  return normalizeLocale(navigator.language);
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    "zh-CN": { translation: zhCN },
  },
  lng: detectInitialLocale(),
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
  localStorage.setItem(LOCALE_STORAGE_KEY, lng);
});

document.documentElement.lang = i18n.language;

export function setLocale(locale: Locale) {
  void i18n.changeLanguage(locale);
}

export default i18n;
