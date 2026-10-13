import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { WorkflowData, WorkflowEntry } from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";
import { formatSpeed, sanitizeUrlForDisplay } from "@/lib/utils";
import type { WorkflowKind } from "@/lib/workflow-library";
import { validateWorkflowEntry } from "@/lib/workflow-library";
import { useWorkflowStore } from "@/stores/workflow-store";

const TITLE = {
  task_preset: "workflow.taskPreset",
  site_profile: "workflow.siteProfile",
  saved_view: "workflow.savedView",
} as const satisfies Record<WorkflowKind, TranslationKey>;
const HINT = {
  task_preset: "workflow.presetHint",
  site_profile: "workflow.siteHint",
  saved_view: "workflow.viewHint",
} as const satisfies Record<WorkflowKind, TranslationKey>;
const SORT_KEYS = {
  updated_at: "taskList.sortUpdatedDesc",
  created_at: "taskList.sortCreatedDesc",
  file_size: "taskList.sortSizeDesc",
  progress: "taskList.sortProgressDesc",
  speed: "taskList.sortSpeedDesc",
  status: "taskList.sortStatusAsc",
  queue_order: "workflow.queueOrder",
} as const satisfies Record<string, TranslationKey>;
const FILE_TYPE_KEYS = {
  archive: "taskList.fileTypeArchive",
  image: "taskList.fileTypeImage",
  video: "taskList.fileTypeVideo",
  document: "taskList.fileTypeDocument",
  app: "taskList.fileTypeApp",
  other: "taskList.fileTypeOther",
} as const satisfies Record<string, TranslationKey>;

export function WorkflowPicker({
  kind,
  getData,
  onApply,
  accepts,
  disabled = false,
  allowApply = true,
}: {
  kind: WorkflowKind;
  getData: () => WorkflowData;
  onApply: (data: WorkflowData) => void;
  accepts?: (entry: WorkflowEntry) => boolean;
  disabled?: boolean;
  allowApply?: boolean;
}) {
  const { t } = useTranslation();
  const id = useId();
  const store = useWorkflowStore();
  const [selectedId, setSelectedId] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [confirmApply, setConfirmApply] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmUpdate, setConfirmUpdate] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const entries = store.entries.filter((entry) => entry.data.kind === kind && (!accepts || accepts(entry)));
  const selected = entries.find((entry) => entry.id === selectedId);
  useEffect(() => {
    if (expanded) void useWorkflowStore.getState().load();
  }, [expanded]);

  async function mutate(action: () => Promise<void>) {
    setBusy(true);
    setError(false);
    try {
      await action();
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="min-w-0 text-xs" onToggle={(event) => setExpanded(event.currentTarget.open)}>
      <summary className="min-h-8 cursor-pointer rounded py-1.5 font-medium text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary">
        {t(TITLE[kind])}
      </summary>
      {expanded ? (
        <div className="space-y-2 py-2">
          <p className="text-text-muted">{t(HINT[kind])}</p>
          <label htmlFor={`${id}-select`} className="sr-only">
            {t(TITLE[kind])}
          </label>
          <select
            id={`${id}-select`}
            value={selected?.id ?? ""}
            disabled={disabled || busy || store.loading}
            className="h-8 w-full rounded-md border border-border-subtle bg-surface-root px-2 text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
            onChange={(event) => {
              const entry = entries.find((row) => row.id === event.target.value);
              setSelectedId(entry?.id ?? "");
              setName(entry?.name ?? "");
              setConfirmApply(false);
              setConfirmDelete(false);
              setConfirmUpdate(false);
              setError(false);
            }}
          >
            <option value="">{store.loading ? t("taskList.loading") : t("workflow.choose")}</option>
            {entries.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.name}
              </option>
            ))}
          </select>
          {selected ? (
            <p className="break-words text-text-secondary">
              {selected.data.kind === "site_profile"
                ? selected.data.value.origin
                : selected.data.kind === "task_preset"
                  ? [
                      selected.data.value.saveDir,
                      t(`queueCenter.priority.${selected.data.value.priority}`),
                      selected.data.value.categoryKey,
                      selected.data.value.taskSpeedLimitBps
                        ? formatSpeed(Number(selected.data.value.taskSpeedLimitBps))
                        : t("workflow.default"),
                    ]
                      .filter(Boolean)
                      .join(" · ")
                  : [
                      t(`nav.${selected.data.value.nav}` as TranslationKey),
                      selected.data.value.search ? sanitizeUrlForDisplay(selected.data.value.search) : "",
                      selected.data.value.source !== "all" ? selected.data.value.source : "",
                      selected.data.value.fileType !== "all"
                        ? t(FILE_TYPE_KEYS[selected.data.value.fileType as keyof typeof FILE_TYPE_KEYS])
                        : "",
                      selected.data.value.failure !== "all"
                        ? t(`taskList.failure_${selected.data.value.failure}`, {
                            defaultValue: selected.data.value.failure,
                          })
                        : "",
                      selected.data.value.resume !== "all"
                        ? t(
                            selected.data.value.resume === "resumable"
                              ? "taskList.resumable"
                              : "taskList.singleConnection",
                          )
                        : "",
                      `${t(SORT_KEYS[selected.data.value.sortKey as keyof typeof SORT_KEYS])} ${selected.data.value.sortDirection === "asc" ? "↑" : "↓"}`,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
            </p>
          ) : null}
          <label htmlFor={`${id}-name`} className="sr-only">
            {t("workflow.name")}
          </label>
          <Input
            id={`${id}-name`}
            className="h-8"
            maxLength={80}
            value={name}
            disabled={disabled || busy}
            placeholder={t("workflow.name")}
            onChange={(event) => setName(event.target.value)}
          />
          <div className="flex flex-wrap gap-1.5">
            {allowApply ? (
              <Button
                size="sm"
                variant="outline"
                type="button"
                disabled={!selected || disabled || busy}
                onClick={() => {
                  if (!selected) return;
                  if (kind !== "saved_view" && !confirmApply) {
                    setConfirmApply(true);
                    return;
                  }
                  try {
                    validateWorkflowEntry(selected);
                    onApply(selected.data);
                    setError(false);
                    setConfirmApply(false);
                  } catch {
                    setError(true);
                  }
                }}
              >
                {t(confirmApply ? "workflow.applyReplace" : "workflow.apply")}
              </Button>
            ) : null}
            <Button
              size="sm"
              variant="outline"
              type="button"
              disabled={!name.trim() || disabled || busy || store.error}
              onClick={() =>
                void mutate(async () => {
                  const entry = { id: crypto.randomUUID(), name: name.trim(), data: getData() };
                  await store.save(entry);
                  setSelectedId(entry.id);
                  setConfirmApply(false);
                })
              }
            >
              {t("workflow.saveCopy")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              type="button"
              disabled={!selected || disabled || busy}
              onClick={() => {
                if (!confirmUpdate) {
                  setConfirmUpdate(true);
                  return;
                }
                void mutate(async () => {
                  if (selected) {
                    await store.save({ ...selected, data: getData() });
                    setConfirmUpdate(false);
                  }
                });
              }}
            >
              {t(confirmUpdate ? "workflow.updateConfirm" : "workflow.update")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              type="button"
              disabled={!selected || !name.trim() || disabled || busy}
              onClick={() =>
                void mutate(async () => {
                  if (selected) await store.save({ ...selected, name: name.trim() });
                })
              }
            >
              {t("workflow.rename")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              type="button"
              disabled={!selected || disabled || busy}
              onClick={() => {
                if (!confirmDelete) {
                  setConfirmDelete(true);
                  return;
                }
                void mutate(async () => {
                  if (selected) {
                    await store.remove(selected.id);
                    setSelectedId("");
                    setName("");
                    setConfirmDelete(false);
                  }
                });
              }}
            >
              {t(confirmDelete ? "workflow.deleteConfirm" : "workflow.delete")}
            </Button>
          </div>
          {busy ? <p role="status">{t("taskList.loading")}</p> : null}
          {error || store.error ? (
            <p role="alert" className="text-status-danger">
              {t("workflow.error")}
              {store.error ? (
                <Button type="button" size="sm" variant="ghost" onClick={() => void store.load()}>
                  {t("workflow.reload")}
                </Button>
              ) : null}
            </p>
          ) : null}
          {store.unsupportedCount > 0 ? (
            <p role="status" className="text-text-muted">
              {t("workflow.unsupported")}
            </p>
          ) : null}
        </div>
      ) : null}
    </details>
  );
}
