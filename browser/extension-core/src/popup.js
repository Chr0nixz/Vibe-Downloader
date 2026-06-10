const log = createLogger("popup");
const api = globalThis.browser ?? globalThis.chrome;
const button = document.querySelector("#send-current");
const status = document.querySelector("#status");
const recentTasks = document.querySelector("#recent-tasks");
const liveTasks = document.querySelector("#live-tasks");
const autoIntercept = document.querySelector("#auto-intercept");
const forwardHeadersMode = document.querySelector("#forward-headers-mode");

void refresh();

button.addEventListener("click", async () => {
  status.textContent = "Sending...";
  button.disabled = true;
  try {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    if (!tab?.url || !/^https?:\/\//i.test(tab.url)) {
      throw new Error("Current tab is not an HTTP download URL.");
    }
    const response = await sendRuntimeMessage({
      type: "vibe-download-current-tab",
      url: tab.url,
      pageUrl: tab.url,
    });
    if (!response?.ok) {
      throw new Error(response?.error ?? "Vibe did not accept the URL.");
    }
    status.textContent = "Sent";
    await refresh();
  } catch (error) {
    log.error("popup handoff failed", error);
    status.textContent = String(error?.message ?? error);
  } finally {
    button.disabled = false;
  }
});

autoIntercept.addEventListener("change", () => {
  void updateSettings({ autoIntercept: autoIntercept.checked });
});

forwardHeadersMode.addEventListener("change", () => {
  void updateSettings({ forwardHeadersMode: forwardHeadersMode.value });
});

async function refresh() {
  try {
    const response = await sendRuntimeMessage({ type: "vibe-popup-status" });
    if (!response?.ok) throw new Error(response?.error ?? "Status unavailable");
    const snapshot = response.status;
    status.textContent = snapshot.bridgeStatus ?? "unknown";
    autoIntercept.checked = snapshot.settings?.autoIntercept !== false;
    forwardHeadersMode.value = snapshot.settings?.forwardHeadersMode ?? (
      snapshot.settings?.forwardHeaders === true ? "enabled" : "ask"
    );
    renderRecent(snapshot.recent ?? []);
    renderLive(snapshot.tasks ?? []);
  } catch (error) {
    log.warn("failed to refresh popup", error);
    status.textContent = "Disconnected";
    renderRecent([]);
    renderLive([]);
  }
}

async function updateSettings(patch) {
  try {
    const response = await sendRuntimeMessage({
      type: "vibe-update-capture-settings",
      patch,
    });
    if (!response?.ok) throw new Error(response?.error ?? "Settings update failed");
    await refresh();
  } catch (error) {
    log.error("settings update failed", error);
    status.textContent = String(error?.message ?? error);
  }
}

function renderRecent(recent) {
  if (!recentTasks) return;
  recentTasks.replaceChildren(
    ...(recent.length > 0 ? recent.map((item) => recentItem(item)) : [emptyItem("No recent tasks")]),
  );
}

function renderLive(tasks) {
  if (!liveTasks) return;
  liveTasks.replaceChildren(
    ...(tasks.length > 0
      ? tasks.map((task) =>
          recentItem({
            url: task.url,
            status: task.status,
            createdAt: task.updatedAt ?? task.createdAt,
            errorMessage: task.errorMessage,
          }),
        )
      : [emptyItem("No live tasks")]),
  );
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

function emptyItem(text) {
  const li = document.createElement("li");
  li.className = "recent-meta";
  li.textContent = text;
  return li;
}

function sendRuntimeMessage(message) {
  return new Promise((resolve, reject) => {
    try {
      const maybePromise = api.runtime.sendMessage(message, (response) => {
        const lastError = api.runtime.lastError;
        if (lastError) reject(new Error(lastError.message));
        else resolve(response);
      });
      if (maybePromise?.then) maybePromise.then(resolve).catch(reject);
    } catch (error) {
      reject(error);
    }
  });
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
