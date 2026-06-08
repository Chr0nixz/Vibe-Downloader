const log = createLogger("popup");
const api = globalThis.browser ?? globalThis.chrome;
const button = document.querySelector("#send-current");
const status = document.querySelector("#status");
const recentTasks = document.querySelector("#recent-tasks");
const RECENT_KEY = "vibeRecentHandoffs";

void renderRecent();

button.addEventListener("click", async () => {
  status.textContent = "Sending...";
  button.disabled = true;
  try {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    if (!tab?.url || !/^https?:\/\//i.test(tab.url)) {
      throw new Error("Current tab is not an HTTP download URL.");
    }
    log.info("sending current tab url", { url: tab.url });
    const response = await api.runtime.sendMessage({
      type: "vibe-download-current-tab",
      url: tab.url,
      pageUrl: tab.url,
    });
    if (!response?.ok) {
      throw new Error(response?.error ?? "Native host did not accept the URL.");
    }
    log.info("download handoff accepted");
    status.textContent = "Sent";
    await renderRecent();
  } catch (error) {
    log.error("popup handoff failed", error);
    status.textContent = String(error?.message ?? error);
    await renderRecent();
  } finally {
    button.disabled = false;
  }
});

async function renderRecent() {
  if (!recentTasks) return;
  try {
    const stored = await api.storage.local.get(RECENT_KEY);
    const recent = Array.isArray(stored?.[RECENT_KEY]) ? stored[RECENT_KEY] : [];
    recentTasks.replaceChildren(
      ...(recent.length > 0
        ? recent.map((item) => recentItem(item))
        : [emptyRecentItem()]),
    );
  } catch (error) {
    log.warn("failed to render recent handoffs", error);
    recentTasks.replaceChildren(emptyRecentItem("Recent tasks unavailable"));
  }
}

function recentItem(item) {
  const li = document.createElement("li");
  const url = document.createElement("div");
  url.className = "recent-url";
  url.textContent = fileNameFromUrl(item.url) ?? item.url;
  url.title = item.url;

  const meta = document.createElement("div");
  meta.className = "recent-meta";
  const statusText = document.createElement("span");
  statusText.textContent = item.status ?? "sent";
  if (item.status === "failed") statusText.className = "recent-failed";
  const time = document.createElement("time");
  time.dateTime = item.createdAt;
  time.textContent = formatTime(item.createdAt);
  meta.append(statusText, time);

  if (item.errorMessage) {
    const error = document.createElement("div");
    error.className = "recent-meta recent-failed";
    error.textContent = item.errorMessage;
    li.append(url, meta, error);
    return li;
  }

  li.append(url, meta);
  return li;
}

function emptyRecentItem(text = "No recent tasks") {
  const li = document.createElement("li");
  li.className = "recent-meta";
  li.textContent = text;
  return li;
}

function fileNameFromUrl(value) {
  try {
    const url = new URL(value);
    return decodeURIComponent(url.pathname.split("/").filter(Boolean).at(-1) ?? "");
  } catch {
    return null;
  }
}

function formatTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
