importScripts("logger.js");

const log = createLogger("background");
const HOST_NAME = "com.vibe_downloader.native_host";
const BROWSER_KIND = "__VIBE_BROWSER_KIND__";
const RECENT_KEY = "vibeRecentHandoffs";

const api = globalThis.browser ?? globalThis.chrome;

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
  log.info("context menu download requested", { url });
  sendDownloadUrl({
    url,
    pageUrl: tab?.url ?? null,
    referrer: tab?.url ?? null,
    suggestedFileName: suggestedFileNameFromUrl(url),
  });
});

api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "vibe-download-current-tab") return false;
  log.info("popup download requested", { url: message.url });
  sendDownloadUrl({
    url: message.url,
    pageUrl: message.pageUrl ?? message.url,
    referrer: message.pageUrl ?? message.url,
    suggestedFileName: suggestedFileNameFromUrl(message.url),
  })
    .then((response) => sendResponse({ ok: true, response }))
    .catch((error) => {
      log.error("popup download failed", error);
      sendResponse({ ok: false, error: String(error?.message ?? error) });
    });
  return true;
});

async function sendDownloadUrl({ url, pageUrl, referrer, suggestedFileName }) {
  const payload = {
    version: 1,
    requestId: crypto.randomUUID(),
    browser: BROWSER_KIND,
    action: "download_url",
    url,
    pageUrl,
    referrer,
    userAgent: navigator.userAgent,
    suggestedFileName,
  };

  log.debug("sending native message", {
    requestId: payload.requestId,
    url,
    browser: BROWSER_KIND,
  });

  return new Promise((resolve, reject) => {
    api.runtime.sendNativeMessage(HOST_NAME, payload, (response) => {
      const lastError = api.runtime.lastError;
      if (lastError) {
        log.error("native messaging failed", {
          requestId: payload.requestId,
          error: lastError.message,
        });
        recordRecentHandoff(payload, "failed", lastError.message);
        reject(new Error(lastError.message));
        return;
      }
      log.info("native host response received", {
        requestId: payload.requestId,
        status: response?.status,
      });
      recordRecentHandoff(payload, response?.status ?? "accepted", null);
      resolve(response);
    });
  });
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
