const api = globalThis.browser ?? globalThis.chrome;
const i18n = (key, subs) => api.i18n.getMessage(key, subs) || key;

const statusBadge = document.getElementById("status-badge");
const connectionInfo = document.getElementById("connection-info");
const captureDisabled = document.getElementById("capture-disabled");
const captureControls = document.getElementById("capture-controls");
const autoInterceptEl = document.getElementById("auto-intercept");
const headersModeEl = document.getElementById("headers-mode");
const fileExtensionsEl = document.getElementById("file-extensions");
const minSizeEl = document.getElementById("min-size");
const saveButton = document.getElementById("save-settings");
const rulesEmpty = document.getElementById("rules-empty");
const rulesList = document.getElementById("rules-list");
const addRuleButton = document.getElementById("add-rule");
const ruleForm = document.getElementById("rule-form");
const ruleFormTitle = document.getElementById("rule-form-title");
const ruleHostEl = document.getElementById("rule-host");
const ruleSubdomainsEl = document.getElementById("rule-subdomains");
const ruleModeEl = document.getElementById("rule-mode");
const ruleExtensionsEl = document.getElementById("rule-extensions");
const ruleMinSizeEl = document.getElementById("rule-min-size");
const ruleHeadersEl = document.getElementById("rule-headers");
const saveRuleButton = document.getElementById("save-rule");
const cancelRuleButton = document.getElementById("cancel-rule");

let dirty = false;
let currentRules = [];
let editingRuleIndex = -1;

function refresh() {
  api.runtime.sendMessage({ type: "vibe-popup-status" }).then(
    (response) => {
      if (response?.ok) render(response.status);
    },
    (error) => {
      connectionInfo.textContent = `Failed to communicate with the extension: ${String(error?.message ?? error)}`;
    },
  );
}

function render(status) {
  const badge = status.bridgeStatus ?? "disconnected";
  statusBadge.textContent = badge.charAt(0).toUpperCase() + badge.slice(1);
  statusBadge.className = `badge ${badge}`;

  if (badge === "connected") {
    connectionInfo.textContent = i18n("connectionConnected");
  } else if (badge === "error") {
    connectionInfo.textContent = i18n("connectionError");
  } else {
    connectionInfo.textContent = i18n("connectionNotConnected");
  }

  if (status.experimentalCapture) {
    captureDisabled.classList.add("hidden");
    captureControls.classList.remove("hidden");
    autoInterceptEl.checked = !!status.settings?.autoIntercept;
    headersModeEl.value = status.settings?.forwardHeadersMode ?? "disabled";
    fileExtensionsEl.value = (status.settings?.fileExtensions ?? []).join(", ");
    minSizeEl.value = Number(status.settings?.minSizeBytes ?? 0) || 0;
    saveButton.disabled = !dirty;
  } else {
    captureDisabled.classList.remove("hidden");
    captureControls.classList.add("hidden");
  }

  const rules = status.settings?.siteRules ?? [];
  currentRules = rules;
  if (rules.length === 0) {
    rulesEmpty.classList.remove("hidden");
    rulesList.replaceChildren();
  } else {
    rulesEmpty.classList.add("hidden");
    // Build list items via DOM API (not innerHTML) to avoid XSS on dynamic fields.
    const fragment = document.createDocumentFragment();
    rules.forEach((rule, index) => {
      const li = document.createElement("li");

      const info = document.createElement("div");
      info.className = "rule-info";

      const strong = document.createElement("strong");
      strong.textContent = rule.hostPattern ?? "";
      info.appendChild(strong);

      info.appendChild(document.createTextNode(` — mode: ${rule.mode ?? "default"}`));
      if (rule.forwardHeaders != null) {
        info.appendChild(
          document.createTextNode(`, headers: ${rule.forwardHeaders ? "on" : "off"}`),
        );
      }

      const actions = document.createElement("div");
      actions.className = "rule-actions";

      const editButton = document.createElement("button");
      editButton.type = "button";
      editButton.className = "secondary";
      editButton.dataset.edit = String(index);
      editButton.textContent = i18n("editButton");

      const deleteButton = document.createElement("button");
      deleteButton.type = "button";
      deleteButton.className = "danger";
      deleteButton.dataset.delete = String(index);
      deleteButton.textContent = i18n("deleteButton");

      actions.appendChild(editButton);
      actions.appendChild(deleteButton);

      li.appendChild(info);
      li.appendChild(actions);
      fragment.appendChild(li);
    });
    rulesList.replaceChildren(fragment);
  }
}

/* ── Site rules CRUD ── */

rulesList.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  const editIndex = target.dataset.edit;
  const deleteIndex = target.dataset.delete;
  if (editIndex != null) {
    openRuleForm(Number(editIndex));
  } else if (deleteIndex != null) {
    deleteRule(Number(deleteIndex));
  }
});

addRuleButton.addEventListener("click", () => openRuleForm(-1));

cancelRuleButton.addEventListener("click", closeRuleForm);

saveRuleButton.addEventListener("click", () => {
  const hostPattern = ruleHostEl.value.trim();
  if (!hostPattern) {
    ruleHostEl.focus();
    return;
  }
  const extensions = ruleExtensionsEl.value
    .split(",")
    .map((ext) => ext.trim().toLowerCase())
    .filter(Boolean);
  const minSizeRaw = ruleMinSizeEl.value;
  const rule = {
    id: editingRuleIndex >= 0 ? currentRules[editingRuleIndex]?.id ?? crypto.randomUUID() : crypto.randomUUID(),
    hostPattern,
    includeSubdomains: ruleSubdomainsEl.checked,
    mode: ruleModeEl.value,
    fileExtensions: extensions,
    minSizeBytes: minSizeRaw ? String(Number(minSizeRaw) || 0) : null,
    forwardHeaders: ruleHeadersEl.value === "" ? null : ruleHeadersEl.value === "true",
  };
  const next = [...currentRules];
  if (editingRuleIndex >= 0) {
    next[editingRuleIndex] = rule;
  } else {
    next.push(rule);
  }
  saveRules(next);
  closeRuleForm();
});

function openRuleForm(index) {
  editingRuleIndex = index;
  if (index >= 0) {
    const rule = currentRules[index];
    ruleFormTitle.textContent = i18n("editRule");
    ruleHostEl.value = rule?.hostPattern ?? "";
    ruleSubdomainsEl.checked = !!rule?.includeSubdomains;
    ruleModeEl.value = rule?.mode ?? "auto";
    ruleExtensionsEl.value = (rule?.fileExtensions ?? []).join(", ");
    ruleMinSizeEl.value = rule?.minSizeBytes ?? "";
    ruleHeadersEl.value = rule?.forwardHeaders == null ? "" : String(rule.forwardHeaders);
  } else {
    ruleFormTitle.textContent = i18n("newRule");
    ruleHostEl.value = "";
    ruleSubdomainsEl.checked = false;
    ruleModeEl.value = "auto";
    ruleExtensionsEl.value = "";
    ruleMinSizeEl.value = "";
    ruleHeadersEl.value = "";
  }
  ruleForm.classList.remove("hidden");
  addRuleButton.classList.add("hidden");
  ruleHostEl.focus();
}

function closeRuleForm() {
  ruleForm.classList.add("hidden");
  addRuleButton.classList.remove("hidden");
  editingRuleIndex = -1;
}

function deleteRule(index) {
  const rule = currentRules[index];
  if (!confirm(i18n("deleteRuleConfirm", [rule?.hostPattern ?? i18n("unknownHost")]))) return;
  const next = currentRules.filter((_, i) => i !== index);
  saveRules(next);
}

function saveRules(rules) {
  api.runtime
    .sendMessage({ type: "vibe-update-capture-settings", patch: { siteRules: rules } })
    .then(() => {
      currentRules = rules;
      refresh();
    })
    .catch((error) => {
      alert(`Failed to save rules: ${String(error?.message ?? error)}`);
    });
}

function markDirty() {
  dirty = true;
  saveButton.disabled = false;
}

autoInterceptEl.addEventListener("change", markDirty);
headersModeEl.addEventListener("change", markDirty);
fileExtensionsEl.addEventListener("input", markDirty);
minSizeEl.addEventListener("input", markDirty);

saveButton.addEventListener("click", () => {
  const patch = {
    autoIntercept: autoInterceptEl.checked,
    forwardHeadersMode: headersModeEl.value,
    fileExtensions: fileExtensionsEl.value
      .split(",")
      .map((ext) => ext.trim().toLowerCase())
      .filter(Boolean),
    minSizeBytes: String(Number(minSizeEl.value) || 0),
  };
  api.runtime
    .sendMessage({ type: "vibe-update-capture-settings", patch })
    .then(() => {
      dirty = false;
      saveButton.disabled = true;
      saveButton.textContent = i18n("savedButton");
      setTimeout(() => {
        saveButton.textContent = i18n("saveSettings");
      }, 2_000);
    })
    .catch((error) => {
      saveButton.textContent = `Error: ${String(error?.message ?? error)}`;
    });
});

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

refresh();
