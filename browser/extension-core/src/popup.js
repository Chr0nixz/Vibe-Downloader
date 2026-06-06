const api = globalThis.browser ?? globalThis.chrome;
const button = document.querySelector("#send-current");
const status = document.querySelector("#status");

button.addEventListener("click", async () => {
  status.textContent = "Sending...";
  button.disabled = true;
  try {
    const [tab] = await api.tabs.query({ active: true, currentWindow: true });
    if (!tab?.url || !/^https?:\/\//i.test(tab.url)) {
      throw new Error("Current tab is not an HTTP download URL.");
    }
    const response = await api.runtime.sendMessage({
      type: "vibe-download-current-tab",
      url: tab.url,
      pageUrl: tab.url,
    });
    if (!response?.ok) {
      throw new Error(response?.error ?? "Native host did not accept the URL.");
    }
    status.textContent = "Sent";
  } catch (error) {
    status.textContent = String(error?.message ?? error);
  } finally {
    button.disabled = false;
  }
});
