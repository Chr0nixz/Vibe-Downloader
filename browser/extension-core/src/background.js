const HOST_NAME = "com.vibe_downloader.native_host";
const BROWSER_KIND = "__VIBE_BROWSER_KIND__";

const api = globalThis.browser ?? globalThis.chrome;

api.runtime.onInstalled.addListener(() => {
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
  if (!url) return;
  sendDownloadUrl({
    url,
    pageUrl: tab?.url ?? null,
    referrer: tab?.url ?? null,
    suggestedFileName: suggestedFileNameFromUrl(url),
  });
});

api.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "vibe-download-current-tab") return false;
  sendDownloadUrl({
    url: message.url,
    pageUrl: message.pageUrl ?? message.url,
    referrer: message.pageUrl ?? message.url,
    suggestedFileName: suggestedFileNameFromUrl(message.url),
  })
    .then((response) => sendResponse({ ok: true, response }))
    .catch((error) => sendResponse({ ok: false, error: String(error?.message ?? error) }));
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

  return new Promise((resolve, reject) => {
    api.runtime.sendNativeMessage(HOST_NAME, payload, (response) => {
      const lastError = api.runtime.lastError;
      if (lastError) {
        reject(new Error(lastError.message));
        return;
      }
      resolve(response);
    });
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
