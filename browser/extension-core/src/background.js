importScripts("logger.js");

const log = createLogger("background");
const HOST_NAME = "com.vibe_downloader.native_host";
const BROWSER_KIND = "__VIBE_BROWSER_KIND__";
const EXPERIMENTAL_CAPTURE = "__VIBE_EXPERIMENTAL_CAPTURE__" === "true";
const RECENT_KEY = "vibeRecentHandoffs";
const SETTINGS_KEY = "vibeBrowserCaptureSettings";
const HEADER_CACHE_TTL_MS = 30_000;
const MAX_HEADER_CACHE = 80;
const MAX_LIVE_TASK_CACHE = 120;
const ACTIVE_TASK_STATUSES = new Set([
  "queued",
  "downloading",
  "retrying",
  "paused",
  "waiting_network",
  "needs_attention",
]);
const ALLOWED_HEADER_NAMES = new Set([
  "accept",
  "accept-language",
  "cache-control",
  "cookie",
  "dnt",
  "origin",
  "pragma",
  "referer",
  "user-agent",
]);

const api = globalThis.browser ?? globalThis.chrome;
const headerCache = new Map();
const pendingWs = new Map();
const liveTasks = new Map();

let ws = null;
let bridge = null;
let reconnectTimer = null;
let lastStatus = "disconnected";
let captureSettingsCache = normalizeCaptureSettings(null);

api.runtime.onInstalled.addListener(() => {
  log.info("extension installed, registering context menus");
  api.contextMenus.create({
    id: "vibe-download-link",
    title: "Download with Vibe Downloader",
    contexts: ["link"],
  });
  api.contextMenus.create({
    id: "vibe-download-selection",
    title: "Download selected URL with Vibe Downloader",
    contexts: ["selection"],
  });
});

api.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl ?? firstUrlFromText(info.selectionText ?? "");
  if (!url) {
    log.warn("context menu click without a valid url");
    return;
  }
  void sendDownloadUrl({
    url,
    pageUrl: tab?.url ?? null,
    referrer: tab?.url ?? null,
    suggestedFileName: suggestedFileNameFromUrl(url),
    source: "browser_manual",
  });
});

api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "vibe-download-current-tab") {
    sendDownloadUrl({
      url: message.url,
      pageUrl: message.pageUrl ?? message.url,
      referrer: message.pageUrl ?? message.url,
      suggestedFileName: suggestedFileNameFromUrl(message.url),
      source: "browser_manual",
    })
      .then((response) => sendResponse({ ok: true, response }))
      .catch((error) => sendResponse({ ok: false, error: String(error?.message ?? error) }));
    return true;
  }

  if (message?.type === "vibe-popup-status") {
    popupStatus()
      .then((status) => sendResponse({ ok: true, status }))
      .catch((error) => sendResponse({ ok: false, error: String(error?.message ?? error) }));
    return true;
  }

  if (message?.type === "vibe-update-capture-settings") {
    updateCaptureSettings(message.patch ?? {})
      .then((settings) => sendResponse({ ok: true, settings }))
      .catch((error) => sendResponse({ ok: false, error: String(error?.message ?? error) }));
    return true;
  }

  return false;
});

if (EXPERIMENTAL_CAPTURE && api.downloads?.onCreated) {
  api.downloads.onCreated.addListener((download) => {
    void handleBrowserDownload(download);
  });
}

if (EXPERIMENTAL_CAPTURE && api.webRequest?.onBeforeSendHeaders) {
  try {
    api.webRequest.onBeforeSendHeaders.addListener(
      cacheRequestHeaders,
      { urls: ["http://*/*", "https://*/*"] },
      ["requestHeaders", "extraHeaders"],
    );
  } catch {
    api.webRequest.onBeforeSendHeaders.addListener(
      cacheRequestHeaders,
      { urls: ["http://*/*", "https://*/*"] },
      ["requestHeaders"],
    );
  }
}

void ensureBridge();

async function handleBrowserDownload(download) {
  if (!EXPERIMENTAL_CAPTURE) return;
  if (!download?.url || !/^https?:\/\//i.test(download.url)) return;
  const settings = await getCaptureSettings();
  const decision = shouldIntercept(download, settings);
  if (!decision.intercept) {
    log.debug("browser download skipped", { url: download.url, reason: decision.reason });
    return;
  }

  try {
    await callApi(api.downloads.pause, download.id);
  } catch (error) {
    log.warn("failed to pause browser download before handoff", error);
  }

  try {
    const headerConsent = headerForwardingDecision(download.url, settings);
    const headers = headerConsent.forward ? await forwardedHeaders(download.url) : [];
    const result = await sendDownloadUrl({
      url: download.url,
      pageUrl: download.referrer ?? null,
      referrer: download.referrer ?? null,
      suggestedFileName: fileNameFromPath(download.filename) ?? suggestedFileNameFromUrl(download.url),
      source: "browser_auto",
      browserDownloadId: String(download.id),
      totalBytes: Number.isFinite(download.totalBytes) && download.totalBytes > 0
        ? String(download.totalBytes)
        : null,
      mime: download.mime ?? null,
      forwardedHeaders: headers,
      headersAvailable: headerConsent.available || headers.length > 0,
      headerConsentState: headerConsent.state,
    });
    if (result?.status === "failed") throw new Error(result.errorMessage ?? "Vibe rejected the download.");
    await callApi(api.downloads.cancel, download.id);
    try {
      await callApi(api.downloads.erase, { id: download.id });
    } catch {
      /* erase is best effort */
    }
  } catch (error) {
    log.error("auto handoff failed, resuming browser download", error);
    try {
      await callApi(api.downloads.resume, download.id);
    } catch (resumeError) {
      log.warn("failed to resume browser download", resumeError);
    }
  }
}

async function sendDownloadUrl({
  url,
  pageUrl,
  referrer,
  suggestedFileName,
  source,
  browserDownloadId = null,
  totalBytes = null,
  mime = null,
  forwardedHeaders = [],
  headersAvailable = false,
  headerConsentState = null,
}) {
  const payload = {
    version: 1,
    requestId: crypto.randomUUID(),
    browser: BROWSER_KIND,
    action: "download_url",
    source,
    browserDownloadId,
    url,
    pageUrl,
    referrer,
    userAgent: navigator.userAgent,
    suggestedFileName,
    totalBytes,
    mime,
    forwardedHeaders,
    headersAvailable,
    headerConsentState,
  };

  try {
    await ensureBridge();
    if (ws?.readyState === WebSocket.OPEN) {
      const response = await sendWsRequest("createDownload", payload);
      recordRecentHandoff(payload, response?.status ?? "received", response?.errorMessage ?? null);
      return response;
    }
  } catch (error) {
    log.warn("realtime create failed, falling back to native messaging", error);
  }

  const response = await nativeMessage(payload);
  recordRecentHandoff(payload, response?.status ?? "accepted", response?.errorMessage ?? null);
  return response;
}

async function ensureBridge() {
  if (ws?.readyState === WebSocket.OPEN) return;
  if (!bridge) {
    const response = await nativeMessage({ action: "bootstrap" });
    if (!response?.wsUrl || !response?.token) {
      throw new Error(response?.errorMessage ?? "Vibe realtime bridge is unavailable.");
    }
    bridge = { wsUrl: response.wsUrl, token: response.token };
  }
  await connectWs();
}

function connectWs() {
  return new Promise((resolve, reject) => {
    if (!bridge) {
      reject(new Error("Bridge bootstrap is missing."));
      return;
    }
    const socket = new WebSocket(`${bridge.wsUrl}?token=${encodeURIComponent(bridge.token)}`);
    ws = socket;
    socket.addEventListener("open", () => {
      lastStatus = "connected";
      resolve();
      void sendWsRequest("getSettings", null)
        .then((settings) => saveLocalCaptureSettings(settings))
        .catch((error) => log.warn("failed to sync capture settings", error));
    });
    socket.addEventListener("message", (event) => handleWsMessage(event.data));
    socket.addEventListener("close", () => {
      lastStatus = "disconnected";
      ws = null;
      rejectPendingWsRequests("Realtime bridge disconnected.");
      scheduleReconnect();
    });
    socket.addEventListener("error", () => {
      lastStatus = "error";
      reject(new Error("Vibe realtime bridge connection failed."));
    });
  });
}

function handleWsMessage(raw) {
  let message;
  try {
    message = JSON.parse(raw);
  } catch {
    return;
  }
  if (message.type === "result") {
    const pending = pendingWs.get(message.payload?.id);
    if (pending) {
      pendingWs.delete(message.payload.id);
      clearTimeout(pending.timeoutId);
      pending.resolve(message.payload.value);
    }
    return;
  }
  if (message.type === "error") {
    for (const [id, pending] of pendingWs) {
      pendingWs.delete(id);
      clearTimeout(pending.timeoutId);
      pending.reject(new Error(message.payload?.message ?? "Realtime request failed."));
      break;
    }
    return;
  }
  if (message.type === "tasksSnapshot") {
    liveTasks.clear();
    for (const task of message.payload?.tasks ?? []) upsertLiveTask(task);
    return;
  }
  if (message.type === "taskUpdated") {
    const task = message.payload;
    if (task?.id) upsertLiveTask(task);
    return;
  }
  if (message.type === "taskProgress") {
    const payload = message.payload;
    const current = liveTasks.get(payload?.taskId);
    if (current) {
      upsertLiveTask({
        ...current,
        ...payload,
        id: payload.taskId,
        updatedAt: new Date().toISOString(),
      });
    }
  }
}

function upsertLiveTask(task) {
  if (!task?.id) return;
  liveTasks.delete(task.id);
  liveTasks.set(task.id, task);
  pruneLiveTasks();
}

function pruneLiveTasks() {
  if (liveTasks.size <= MAX_LIVE_TASK_CACHE) return;
  const ranked = Array.from(liveTasks.values()).sort((left, right) => {
    const leftActive = ACTIVE_TASK_STATUSES.has(left.status) ? 1 : 0;
    const rightActive = ACTIVE_TASK_STATUSES.has(right.status) ? 1 : 0;
    if (leftActive !== rightActive) return rightActive - leftActive;
    return taskUpdatedAt(right) - taskUpdatedAt(left);
  });
  liveTasks.clear();
  for (const task of ranked.slice(0, MAX_LIVE_TASK_CACHE)) {
    liveTasks.set(task.id, task);
  }
}

function taskUpdatedAt(task) {
  const parsed = Date.parse(task?.updatedAt ?? task?.createdAt ?? "");
  return Number.isFinite(parsed) ? parsed : 0;
}

function sendWsRequest(type, payload) {
  return new Promise((resolve, reject) => {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      reject(new Error("Realtime bridge is not connected."));
      return;
    }
    const id = crypto.randomUUID();
    const timeoutId = setTimeout(() => {
      const pending = pendingWs.get(id);
      if (!pending) return;
      pendingWs.delete(id);
      pending.reject(new Error("Realtime request timed out."));
    }, 10_000);
    pendingWs.set(id, { resolve, reject, timeoutId });
    ws.send(JSON.stringify({ id, type, payload }));
  });
}

function rejectPendingWsRequests(message) {
  for (const [id, pending] of pendingWs) {
    pendingWs.delete(id);
    clearTimeout(pending.timeoutId);
    pending.reject(new Error(message));
  }
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    void ensureBridge().catch((error) => log.debug("bridge reconnect failed", error));
  }, 2_000);
}

async function nativeMessage(payload) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const done = (response) => {
      if (settled) return;
      settled = true;
      const lastError = api.runtime.lastError;
      if (lastError) {
        reject(new Error(lastError.message));
      } else if (response?.status === "failed") {
        reject(new Error(response.errorMessage ?? "Native host failed."));
      } else {
        resolve(response);
      }
    };
    try {
      const maybePromise = api.runtime.sendNativeMessage(HOST_NAME, payload, done);
      if (maybePromise?.then) {
        maybePromise.then(done).catch((error) => {
          if (!settled) reject(error);
        });
      }
    } catch (error) {
      reject(error);
    }
  });
}

function cacheRequestHeaders(details) {
  if (!EXPERIMENTAL_CAPTURE) return {};
  cleanupHeaderCache();
  const headerConsent = headerForwardingDecision(details.url, captureSettingsCache);
  if (!headerConsent.forward) return {};
  const headers = (details.requestHeaders ?? [])
    .map((header) => ({
      name: header.name,
      value: header.value ?? "",
    }))
    .filter((header) => ALLOWED_HEADER_NAMES.has(header.name.toLowerCase()) && header.value);
  headerCache.set(details.url, {
    headers,
    createdAt: Date.now(),
  });
  return {};
}

async function forwardedHeaders(url) {
  if (!EXPERIMENTAL_CAPTURE) return [];
  const cached = headerCache.get(url);
  const headers = cached && Date.now() - cached.createdAt < HEADER_CACHE_TTL_MS
    ? [...cached.headers]
    : [];
  const hasCookie = headers.some((header) => header.name.toLowerCase() === "cookie");
  if (!hasCookie && api.cookies?.getAll) {
    try {
      const cookies = await api.cookies.getAll({ url });
      const value = cookies.map((cookie) => `${cookie.name}=${cookie.value}`).join("; ");
      if (value) headers.push({ name: "Cookie", value });
    } catch (error) {
      log.warn("failed to read cookies for download", error);
    }
  }
  return headers;
}

function cleanupHeaderCache() {
  const now = Date.now();
  for (const [key, value] of headerCache) {
    if (now - value.createdAt > HEADER_CACHE_TTL_MS || headerCache.size > MAX_HEADER_CACHE) {
      headerCache.delete(key);
    }
  }
}

async function getCaptureSettings() {
  const stored = await api.storage.local.get(SETTINGS_KEY);
  captureSettingsCache = normalizeCaptureSettings(stored?.[SETTINGS_KEY]);
  return captureSettingsCache;
}

async function saveLocalCaptureSettings(settings) {
  const normalized = normalizeCaptureSettings(settings);
  captureSettingsCache = normalized;
  await api.storage.local.set({ [SETTINGS_KEY]: normalized });
  return normalized;
}

async function updateCaptureSettings(patch) {
  const current = await getCaptureSettings();
  const next = normalizeCaptureSettings({ ...current, ...patch });
  await saveLocalCaptureSettings(next);
  if (ws?.readyState === WebSocket.OPEN) {
    try {
      return await sendWsRequest("updateSettings", patch);
    } catch (error) {
      log.warn("failed to sync capture settings to app", error);
    }
  }
  return next;
}

function normalizeCaptureSettings(value) {
  if (!EXPERIMENTAL_CAPTURE) {
    return {
      autoIntercept: false,
      forwardHeadersMode: "disabled",
      forwardHeaders: false,
      minSizeBytes: String(Number(value?.minSizeBytes ?? 0) || 0),
      fileExtensions: Array.isArray(value?.fileExtensions)
        ? value.fileExtensions
        : ["zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "pdf", "mp4", "mkv", "m3u8"],
      siteRules: Array.isArray(value?.siteRules) ? value.siteRules : [],
    };
  }
  return {
    autoIntercept: value?.autoIntercept !== false,
    forwardHeadersMode: value?.forwardHeadersMode ??
      (typeof value?.forwardHeaders === "boolean"
        ? value.forwardHeaders ? "enabled" : "disabled"
        : "ask"),
    forwardHeaders: value?.forwardHeadersMode === "enabled" ||
      (value?.forwardHeadersMode == null && value?.forwardHeaders === true),
    minSizeBytes: String(Number(value?.minSizeBytes ?? 0) || 0),
    fileExtensions: Array.isArray(value?.fileExtensions)
      ? value.fileExtensions
      : ["zip", "7z", "rar", "exe", "msi", "dmg", "pkg", "iso", "tar", "gz", "pdf", "mp4", "mkv", "m3u8"],
    siteRules: Array.isArray(value?.siteRules) ? value.siteRules : [],
  };
}

function shouldForwardHeaders(url, settings) {
  return headerForwardingDecision(url, settings).forward;
}

function headerForwardingDecision(url, settings) {
  const parsed = new URL(url);
  const rule = matchingRule(parsed.hostname, settings.siteRules);
  if (typeof rule?.forwardHeaders === "boolean") {
    return {
      forward: rule.forwardHeaders,
      state: rule.forwardHeaders ? "allowed" : "denied",
      available: true,
    };
  }
  if (settings.forwardHeadersMode === "enabled") {
    return { forward: true, state: "allowed", available: true };
  }
  if (settings.forwardHeadersMode === "ask") {
    return { forward: false, state: "ask", available: true };
  }
  return { forward: false, state: "denied", available: false };
}

function shouldIntercept(download, settings) {
  if (!settings.autoIntercept) return { intercept: false, reason: "disabled" };
  const url = new URL(download.url);
  const rule = matchingRule(url.hostname, settings.siteRules);
  if (rule?.mode === "never") return { intercept: false, reason: "site-rule" };
  if (rule?.mode === "ask") return { intercept: false, reason: "ask-rule" };
  const minSize = Number(rule?.minSizeBytes ?? settings.minSizeBytes ?? 0);
  if (download.totalBytes > 0 && Number.isFinite(minSize) && download.totalBytes < minSize) {
    return { intercept: false, reason: "size" };
  }
  const extensions = rule?.fileExtensions?.length ? rule.fileExtensions : settings.fileExtensions;
  if (extensions?.length) {
    const ext = extensionFromUrl(download.url, download.filename);
    if (ext && !extensions.map((value) => value.toLowerCase()).includes(ext)) {
      return { intercept: false, reason: "extension" };
    }
  }
  return { intercept: true };
}

function matchingRule(hostname, rules) {
  return rules.find((rule) => {
    const pattern = String(rule.hostPattern ?? "").toLowerCase();
    const host = hostname.toLowerCase();
    if (!pattern) return false;
    if (pattern.startsWith("*.")) {
      const root = pattern.slice(2);
      return host === root || host.endsWith(`.${root}`);
    }
    return rule.includeSubdomains
      ? host === pattern || host.endsWith(`.${pattern}`)
      : host === pattern;
  });
}

async function popupStatus() {
  const stored = await api.storage.local.get(RECENT_KEY);
  return {
    bridgeStatus: lastStatus,
    experimentalCapture: EXPERIMENTAL_CAPTURE,
    settings: await getCaptureSettings(),
    recent: Array.isArray(stored?.[RECENT_KEY]) ? stored[RECENT_KEY] : [],
    tasks: Array.from(liveTasks.values()).slice(0, 8),
  };
}

async function recordRecentHandoff(payload, status, errorMessage) {
  const item = {
    requestId: payload.requestId,
    url: payload.url,
    status,
    errorMessage,
    createdAt: new Date().toISOString(),
  };
  try {
    const stored = await api.storage.local.get(RECENT_KEY);
    const recent = Array.isArray(stored?.[RECENT_KEY]) ? stored[RECENT_KEY] : [];
    await api.storage.local.set({
      [RECENT_KEY]: [item, ...recent].slice(0, 8),
    });
  } catch (error) {
    log.warn("failed to record recent handoff", error);
  }
}

function callApi(fn, ...args) {
  return new Promise((resolve, reject) => {
    try {
      const maybePromise = fn.call(api.downloads, ...args, (result) => {
        const lastError = api.runtime.lastError;
        if (lastError) reject(new Error(lastError.message));
        else resolve(result);
      });
      if (maybePromise?.then) maybePromise.then(resolve).catch(reject);
    } catch (error) {
      reject(error);
    }
  });
}

function firstUrlFromText(text) {
  const match = text.match(/https?:\/\/[^\s"'<>]+/i);
  return match?.[0] ?? null;
}

function suggestedFileNameFromUrl(value) {
  try {
    const url = new URL(value);
    const last = url.pathname.split("/").filter(Boolean).at(-1);
    return last ? decodeURIComponent(last) : null;
  } catch {
    return null;
  }
}

function fileNameFromPath(value) {
  const name = String(value ?? "").split(/[\\/]/).filter(Boolean).at(-1);
  return name || null;
}

function extensionFromUrl(url, filename) {
  const name = fileNameFromPath(filename) ?? suggestedFileNameFromUrl(url) ?? "";
  const match = name.toLowerCase().match(/\.([a-z0-9_-]{1,12})(?:$|\?)/);
  return match?.[1] ?? null;
}
