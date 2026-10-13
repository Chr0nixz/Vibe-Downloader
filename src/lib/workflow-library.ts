import type {
  BulkTaskOptionsPatch,
  BulkTaskOptionsResult,
  SiteProfile,
  TaskPreset,
  WorkflowData,
  WorkflowEntry,
  WorkflowLibrary,
} from "@/generated/bindings";
import { parseRequestProfile, type RequestProfileDraft } from "@/lib/request-profile";
import { isTauriRuntime } from "@/lib/runtime";

export type WorkflowKind = WorkflowData["kind"];
const STORAGE_KEY = "vibe-workflow-library-v1";
type StoredEntry = { version: number; entry: WorkflowEntry };
const validText = (value: unknown, maxBytes: number): value is string =>
  typeof value === "string" &&
  new TextEncoder().encode(value).length <= maxBytes &&
  !Array.from(value).some(
    (char) => char.charCodeAt(0) < 32 || (char.charCodeAt(0) >= 127 && char.charCodeAt(0) <= 159),
  );

export function workflowError(): Error {
  return new Error("workflow_invalid");
}

export function siteOrigin(value: string): string | null {
  try {
    const url = new URL(value);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.origin : null;
  } catch {
    return null;
  }
}

export function publicSiteProfile(url: string, draft: RequestProfileDraft): SiteProfile {
  const origin = siteOrigin(url);
  const profile = parseRequestProfile(draft);
  if (!origin || !profile) throw workflowError();
  profile.customHeaders = profile.customHeaders.filter(({ name }) => !isSensitiveHeader(name));
  if (profile.referer) {
    const referer = new URL(profile.referer);
    referer.search = "";
    referer.hash = "";
    profile.referer = referer.href;
  }
  if (!profile.userAgent && !profile.referer && !profile.customHeaders.length) throw workflowError();
  return { origin, profile };
}

function isSensitiveHeader(name: string): boolean {
  return /^(x-|authorization$|cookie2?$|proxy-authorization$)/i.test(name);
}

/** Preserve task-only secrets when explicitly replacing the public headers. */
export function applySiteProfile(url: string, current: RequestProfileDraft, site: SiteProfile): RequestProfileDraft {
  if (siteOrigin(url) !== site.origin) throw workflowError();
  const secrets = (parseRequestProfile(current)?.customHeaders ?? []).filter(({ name }) => isSensitiveHeader(name));
  return {
    userAgent: site.profile.userAgent ?? "",
    referer: site.profile.referer ?? "",
    customHeaders: [...site.profile.customHeaders, ...secrets].map(({ name, value }) => `${name}: ${value}`).join("\n"),
  };
}

export function safePreset(preset: TaskPreset): TaskPreset {
  if (
    !preset.saveDir.trim() ||
    preset.saveDir.length > 4096 ||
    Array.from(preset.saveDir).some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127)
  )
    throw workflowError();
  if (preset.proxyMode === "custom") {
    const url = new URL(preset.proxyUrl ?? "");
    if (!["http:", "https:", "socks5:", "socks5h:"].includes(url.protocol) || !url.hostname) throw workflowError();
    url.username = "";
    url.password = "";
    url.search = "";
    url.hash = "";
    if (!["", "/"].includes(url.pathname)) throw workflowError();
    return { ...preset, proxyUrl: url.href };
  }
  return { ...preset, proxyUrl: null, proxyNoProxy: null };
}

function readStored(): StoredEntry[] {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return [];
  const value: unknown = JSON.parse(raw);
  if (!Array.isArray(value) || value.length > 300) throw workflowError();
  return value as StoredEntry[];
}

export function validateWorkflowEntry(entry: WorkflowEntry): void {
  if (
    !entry ||
    Object.keys(entry).some((key) => !["id", "name", "data"].includes(key)) ||
    !validText(entry.name, 240) ||
    !entry.name.trim() ||
    entry.name.length > 80 ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(entry.id)
  )
    throw workflowError();
  const data = entry.data;
  if (
    !data?.value ||
    typeof data.value !== "object" ||
    Object.keys(data).some((key) => !["kind", "value"].includes(key))
  )
    throw workflowError();
  const keys = Object.keys(data.value);
  if (data.kind === "task_preset") {
    const value = data.value;
    if (
      keys.some(
        (key) =>
          ![
            "saveDir",
            "categoryKey",
            "priority",
            "taskSpeedLimitBps",
            "obeySchedule",
            "proxyMode",
            "proxyUrl",
            "proxyNoProxy",
          ].includes(key),
      ) ||
      !["low", "normal", "high"].includes(value.priority) ||
      typeof value.obeySchedule !== "boolean" ||
      !["inherit", "off", "custom"].includes(value.proxyMode) ||
      !validText(value.saveDir, 4096) ||
      (value.categoryKey !== null && !validText(value.categoryKey, 256)) ||
      (value.proxyNoProxy !== null && !validText(value.proxyNoProxy, 4096)) ||
      (value.taskSpeedLimitBps !== null &&
        (!/^[1-9]\d*$/.test(value.taskSpeedLimitBps) || !Number.isSafeInteger(Number(value.taskSpeedLimitBps))))
    )
      throw workflowError();
    const safe = safePreset(value);
    if (safe.proxyUrl !== value.proxyUrl || safe.proxyNoProxy !== value.proxyNoProxy) throw workflowError();
  } else if (data.kind === "site_profile") {
    const value = data.value;
    if (
      !value.profile ||
      !Array.isArray(value.profile.customHeaders) ||
      Object.keys(value.profile).some((key) => !["userAgent", "referer", "customHeaders"].includes(key)) ||
      value.profile.customHeaders.some(
        (header) => !header || Object.keys(header).some((key) => !["name", "value"].includes(key)),
      )
    )
      throw workflowError();
    if (
      keys.some((key) => !["origin", "profile"].includes(key)) ||
      siteOrigin(value.origin) !== value.origin ||
      !Array.isArray(value.profile.customHeaders) ||
      value.profile.customHeaders.some(({ name }) => isSensitiveHeader(name))
    )
      throw workflowError();
    const draft = {
      userAgent: value.profile.userAgent ?? "",
      referer: value.profile.referer ?? "",
      customHeaders: value.profile.customHeaders.map(({ name, value }) => `${name}: ${value}`).join("\n"),
    };
    publicSiteProfile(value.origin, draft);
    if (value.profile.referer && (new URL(value.profile.referer).search || new URL(value.profile.referer).hash))
      throw workflowError();
  } else if (data.kind === "saved_view") {
    const view = data.value;
    if (
      keys.some(
        (key) =>
          !["nav", "search", "sortKey", "sortDirection", "fileType", "source", "failure", "resume"].includes(key),
      ) ||
      !["all", "downloading", "queue", "issues", "attention", "paused", "completed", "failed"].includes(view.nav) ||
      !["updated_at", "created_at", "file_size", "progress", "speed", "status", "queue_order"].includes(view.sortKey) ||
      !["asc", "desc"].includes(view.sortDirection) ||
      !["all", "archive", "image", "video", "document", "app", "other"].includes(view.fileType) ||
      !["all", "resumable", "single_connection"].includes(view.resume) ||
      !validText(view.search, 2048) ||
      !validText(view.source, 512) ||
      ![
        "all",
        "none",
        "network",
        "http",
        "auth",
        "disk_write",
        "remote_changed",
        "temp_file",
        "resume_unavailable",
        "proxy",
        "schedule",
        "hls",
        "dash",
        "metalink",
        "bt",
        "ftp",
        "sftp",
        "webdav",
        "other",
      ].includes(view.failure)
    )
      throw workflowError();
  } else throw workflowError();
}

export async function listWorkflowLibrary(): Promise<WorkflowLibrary> {
  if (isTauriRuntime()) {
    return (await import("@/lib/tauri")).listWorkflowLibrary();
  }
  const stored = readStored();
  const entries = stored.filter((row) => {
    try {
      if (row?.version !== 1) return false;
      validateWorkflowEntry(row.entry);
      return true;
    } catch {
      return false;
    }
  });
  return { entries: entries.map(({ entry }) => entry), unsupportedCount: stored.length - entries.length };
}

export async function saveWorkflowEntry(entry: WorkflowEntry): Promise<void> {
  validateWorkflowEntry(entry);
  if (isTauriRuntime()) {
    await (await import("@/lib/tauri")).saveWorkflowEntry(entry);
    return;
  }
  if (!entry.name.trim() || entry.name.length > 80) throw workflowError();
  const stored = readStored();
  const index = stored.findIndex((row) => row?.entry?.id === entry.id);
  if (index >= 0 && stored[index]?.version !== 1) throw workflowError();
  if (index < 0 && stored.length >= 300) throw workflowError();
  const row = { version: 1, entry: { ...entry, name: entry.name.trim() } };
  if (index < 0) stored.push(row);
  else stored[index] = row;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
}

export async function deleteWorkflowEntry(id: string): Promise<void> {
  if (isTauriRuntime()) {
    await (await import("@/lib/tauri")).deleteWorkflowEntry(id);
    return;
  }
  localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify(readStored().filter((row) => row?.version !== 1 || row.entry?.id !== id)),
  );
}

export async function bulkUpdateTaskOptions(
  ids: string[],
  patch: BulkTaskOptionsPatch,
): Promise<BulkTaskOptionsResult[]> {
  if (isTauriRuntime()) {
    return (await import("@/lib/tauri")).bulkUpdateTaskOptions(ids, patch);
  }
  const adapter = await import("@/lib/tauri-browser");
  return adapter.bulkUpdateTaskOptions(ids, patch);
}

export async function runBulkOptions(
  ids: string[],
  patch: BulkTaskOptionsPatch,
  options: {
    isCancelled: () => boolean;
    onProgress: (results: BulkTaskOptionsResult[]) => void;
    submit?: typeof bulkUpdateTaskOptions;
  },
): Promise<BulkTaskOptionsResult[]> {
  const unique = [...new Set(ids)];
  const results: BulkTaskOptionsResult[] = [];
  for (let index = 0; index < unique.length; index += 100) {
    if (options.isCancelled()) {
      results.push(...unique.slice(index).map((id) => ({ id, status: "not_processed", error: null })));
      break;
    }
    const chunk = unique.slice(index, index + 100);
    try {
      results.push(...(await (options.submit ?? bulkUpdateTaskOptions)(chunk, patch)));
    } catch {
      results.push(...chunk.map((id) => ({ id, status: "failed", error: "workflow_invalid" })));
    }
    options.onProgress([...results]);
  }
  options.onProgress([...results]);
  return results;
}
