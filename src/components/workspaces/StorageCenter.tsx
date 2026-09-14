import { AlertTriangle, HardDrive, Loader2, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  type CleanupReport,
  groupReclaimable,
  groupResumableByTask,
  itemsForMode,
  parseBytes,
  type ResumableTaskGroup,
  STORAGE_KIND_KEYS,
  STORAGE_REASON_KEYS,
  STORAGE_SWEEP_MODE_KEYS,
  summarizeCleanup,
  totalsFor,
} from "@/components/workspaces/storage-center-logic";
import type { CleanupMode, StorageScanResult, StorageSweepRecord } from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import {
  cleanStorageArtifacts,
  cleanupTaskTempFiles,
  getLastStorageSweep,
  onStorageCleanupProgress,
  scanStorage,
} from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { useTaskUIStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";

type ConfirmTarget =
  | {
      kind: "mode";
      mode: CleanupMode;
      label: string;
      count: number;
      bytes: number;
      /** Selected-mode cleanups echo the picked ids; other modes omit them. */
      itemIds?: string[];
    }
  | { kind: "task"; task: ResumableTaskGroup };

/**
 * Storage & Cleanup Center (feature proposal §3.2): per-directory disk
 * overview, categorized reclaimable artifacts with aggregate + selected
 * cleanup, and a read-only view of artifacts kept for resumable tasks.
 *
 * The backend owns every path here — items arrive with opaque ids and the
 * page never builds paths or deletes by itself; it only echoes ids back.
 */
export function StorageCenter() {
  const { t } = useTranslation();
  const setNav = useTaskUIStore((s) => s.setNav);
  const addToast = useToastStore((s) => s.addToast);

  const [scan, setScan] = useState<StorageScanResult | null>(null);
  const [sweep, setSweep] = useState<StorageSweepRecord | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ processed: number; total: number } | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<ConfirmTarget | null>(null);
  const [report, setReport] = useState<CleanupReport | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextScan, nextSweep] = await Promise.all([scanStorage(), getLastStorageSweep()]);
      setScan(nextScan);
      setSweep(nextSweep);
      setSelectedIds(new Set());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | null = null;
    void onStorageCleanupProgress((payload) => {
      setProgress({ processed: payload.processed, total: payload.total });
    }).then((dispose) => {
      unlisten = dispose;
    });
    return () => {
      unlisten?.();
    };
  }, [refresh]);

  const reclaimableGroups = useMemo(() => (scan ? groupReclaimable(scan.items) : []), [scan]);
  const resumableGroups = useMemo(() => (scan ? groupResumableByTask(scan.items) : []), [scan]);
  const totals = useMemo(
    () => (scan ? totalsFor(scan.items) : { reclaimableBytes: 0, reclaimableCount: 0, resumableBytes: 0 }),
    [scan],
  );
  const selectedBytes = useMemo(() => {
    if (!scan) {
      return 0;
    }
    return scan.items.filter((item) => selectedIds.has(item.id)).reduce((sum, item) => sum + parseBytes(item.bytes), 0);
  }, [scan, selectedIds]);

  const runModeCleanup = useCallback(
    async (mode: CleanupMode, itemIds?: string[]) => {
      setBusy(true);
      setProgress({ processed: 0, total: itemIds?.length ?? 0 });
      setReport(null);
      try {
        const result = await cleanStorageArtifacts(mode, itemIds);
        setReport(summarizeCleanup(result));
        addToast({
          tone: result.failedCount > 0 ? "error" : "success",
          title: t("storageCenter.toast.done", { removed: result.removedCount }),
          description:
            result.failedCount > 0
              ? t("storageCenter.toast.failures", { failed: result.failedCount })
              : t("storageCenter.toast.reclaimed", {
                  bytes: formatBytes(Number(result.reclaimedBytes) || 0),
                }),
        });
      } catch (err) {
        addToast({ tone: "error", title: t("storageCenter.toast.failed"), description: errorMessage(err) });
      } finally {
        setBusy(false);
        setProgress(null);
        void refresh();
      }
    },
    [addToast, refresh, t],
  );

  const runTaskCleanup = useCallback(
    async (taskId: string) => {
      setBusy(true);
      setReport(null);
      try {
        const result = await cleanupTaskTempFiles(taskId);
        addToast({
          tone: "info",
          title: t("storageCenter.toast.abandoned"),
          description: t("storageCenter.toast.abandonedHint"),
        });
        setReport(summarizeCleanup(result));
      } catch (err) {
        addToast({ tone: "error", title: t("storageCenter.toast.failed"), description: errorMessage(err) });
      } finally {
        setBusy(false);
        void refresh();
      }
    },
    [addToast, refresh, t],
  );

  const openModeConfirm = (mode: CleanupMode, label: string) => {
    if (!scan) {
      return;
    }
    const targets = itemsForMode(scan.items, mode);
    setConfirm({
      kind: "mode",
      mode,
      label,
      count: targets.length,
      bytes: targets.reduce((sum, item) => sum + parseBytes(item.bytes), 0),
    });
  };

  const executeConfirm = () => {
    if (!confirm) {
      return;
    }
    if (confirm.kind === "mode") {
      void runModeCleanup(confirm.mode, confirm.itemIds);
    } else {
      void runTaskCleanup(confirm.task.taskId);
    }
    setConfirm(null);
  };

  const toggleSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const openSelectedConfirm = () => {
    if (selectedIds.size === 0) {
      return;
    }
    setConfirm({
      kind: "mode",
      mode: "selected",
      label: t("storageCenter.mode.selected"),
      count: selectedIds.size,
      bytes: selectedBytes,
      itemIds: Array.from(selectedIds),
    });
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border-subtle px-3 py-2.5 md:px-4">
        <HardDrive className="h-5 w-5 text-accent-primary" aria-hidden />
        <div className="min-w-0 flex-1">
          <h1 className="text-sm font-semibold text-text-primary">{t("storageCenter.title")}</h1>
          <p className="truncate text-xs text-text-muted">
            {t("storageCenter.subtitle")}
            {sweep ? (
              <>
                {" · "}
                {t("storageCenter.lastSweep", {
                  mode: t(STORAGE_SWEEP_MODE_KEYS[sweep.mode] ?? "storageCenter.sweepMode.startup"),
                  removed: sweep.removedCount,
                  time: formatDateTime(sweep.finishedAt, "dateTime"),
                })}
              </>
            ) : null}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8"
          onClick={() => void refresh()}
          disabled={loading || busy}
        >
          <RotateCcw className={cnIcon(loading)} aria-hidden />
          {t("storageCenter.refresh")}
        </Button>
      </header>

      {error ? (
        <div
          className="flex flex-wrap items-center gap-2 border-b border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger md:px-4"
          role="alert"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-8 shrink-0 border-border-danger text-status-danger"
            onClick={() => void refresh()}
            disabled={loading}
          >
            <RotateCcw className={cnIcon(loading)} aria-hidden />
            {t("storageCenter.retryScan")}
          </Button>
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3 md:px-4">
        {loading ? (
          <div className="space-y-2" role="status" aria-label={t("storageCenter.loading")}>
            {[0, 1, 2].map((row) => (
              <div key={row} className="h-10 animate-pulse rounded-md border border-border-subtle bg-surface-base" />
            ))}
          </div>
        ) : (
          <div className="space-y-5 pb-8">
            {/* Per-directory disk overview. */}
            <section aria-label={t("storageCenter.overviewTitle")} className="space-y-1.5">
              <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {t("storageCenter.overviewTitle")}
              </h2>
              {(scan?.dirs ?? []).map((dir) => (
                <div
                  key={dir.path}
                  className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-md border border-border-subtle bg-surface-base px-3 py-2 text-sm"
                >
                  <span
                    className="min-w-0 max-w-[16rem] truncate font-mono text-xs text-text-secondary"
                    title={dir.path}
                  >
                    {dir.path}
                  </span>
                  <span className="text-xs text-text-muted">
                    {t("storageCenter.diskAvailable", {
                      available: formatBytes(Number(dir.availableBytes) || 0),
                      total: formatBytes(Number(dir.totalBytes) || 0),
                    })}
                  </span>
                  {dir.estimatedCompletableTasks ? (
                    <span className="text-xs text-text-muted">
                      {t("storageCenter.estimatedTasks", { count: dir.estimatedCompletableTasks })}
                    </span>
                  ) : null}
                  <span className="ml-auto text-xs text-status-warning">
                    {t("storageCenter.reclaimableBytes", {
                      bytes: formatBytes(Number(dir.reclaimableBytes) || 0),
                    })}
                  </span>
                  <span className="text-xs text-text-muted">
                    {t("storageCenter.resumableBytes", {
                      bytes: formatBytes(Number(dir.resumableBytes) || 0),
                    })}
                  </span>
                  {dir.truncated ? (
                    <span className="flex items-center gap-1 text-xs text-status-warning" role="status">
                      <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                      {t("storageCenter.truncated")}
                    </span>
                  ) : null}
                </div>
              ))}
            </section>

            {scan && !scanHasItems(scan) ? (
              <div className="rounded-md border border-border-subtle bg-surface-base px-4 py-8 text-center">
                <p className="text-sm text-text-secondary">{t("storageCenter.emptyTitle")}</p>
                <p className="mt-1 text-xs text-text-muted">{t("storageCenter.emptyDescription")}</p>
              </div>
            ) : null}

            {/* Reclaimable artifacts grouped by kind. */}
            {reclaimableGroups.length > 0 ? (
              <section aria-label={t("storageCenter.reclaimableTitle")} className="space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                    {t("storageCenter.reclaimableTitle", {
                      count: totals.reclaimableCount,
                      bytes: formatBytes(totals.reclaimableBytes),
                    })}
                  </h2>
                  <div className="ml-auto flex flex-wrap gap-1.5">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8"
                      disabled={busy || selectedIds.size === 0}
                      onClick={() => openSelectedConfirm()}
                    >
                      <Trash2 className="h-3.5 w-3.5" aria-hidden />
                      {t("storageCenter.cleanSelected", { count: selectedIds.size })}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8"
                      disabled={busy || !itemsForMode(scan?.items ?? [], "orphans").length}
                      onClick={() => openModeConfirm("orphans", t("storageCenter.mode.orphans"))}
                    >
                      {t("storageCenter.mode.orphans")}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8"
                      disabled={busy || !itemsForMode(scan?.items ?? [], "completed_leftovers").length}
                      onClick={() => openModeConfirm("completed_leftovers", t("storageCenter.mode.completed"))}
                    >
                      {t("storageCenter.mode.completed")}
                    </Button>
                    <Button
                      type="button"
                      variant="danger"
                      size="sm"
                      className="h-8"
                      disabled={busy || !itemsForMode(scan?.items ?? [], "all_reclaimable").length}
                      onClick={() => openModeConfirm("all_reclaimable", t("storageCenter.mode.all"))}
                    >
                      {t("storageCenter.mode.all")}
                    </Button>
                  </div>
                </div>
                {busy && progress ? (
                  <p className="text-xs text-text-muted" role="status">
                    {t("storageCenter.cleaning", { processed: progress.processed, total: progress.total })}
                  </p>
                ) : null}
                {report && report.failedCount > 0 ? (
                  <div
                    className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
                    role="alert"
                  >
                    <p className="font-medium">{t("storageCenter.reportFailures", { failed: report.failedCount })}</p>
                    <ul className="mt-1 max-h-24 list-inside list-disc overflow-y-auto">
                      {report.failures.map((failure) => (
                        <li key={failure.itemId} className="truncate font-mono" title={failure.itemId}>
                          {failure.itemId}
                          {failure.errorCode ? ` · ${failure.errorCode}` : ""}
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {reclaimableGroups.map((group) => (
                  <div
                    key={group.kind}
                    className="overflow-hidden rounded-md border border-border-subtle bg-surface-base"
                  >
                    <div className="flex items-center gap-2 border-b border-border-subtle px-3 py-1.5">
                      <span className="text-xs font-medium text-text-secondary">
                        {t(STORAGE_KIND_KEYS[group.kind])}
                      </span>
                      <span className="text-xs text-text-muted">
                        {group.items.length} · {formatBytes(group.totalBytes)}
                      </span>
                    </div>
                    <ul>
                      {group.items.map((item) => (
                        <li
                          key={item.id}
                          className="flex items-center gap-2 border-b border-border-subtle/60 px-3 py-1.5 text-sm last:border-b-0"
                        >
                          <input
                            type="checkbox"
                            className="h-3.5 w-3.5 shrink-0 accent-[var(--accent-primary)]"
                            checked={selectedIds.has(item.id)}
                            onChange={() => toggleSelected(item.id)}
                            aria-label={t("storageCenter.selectItem", { name: item.fileName })}
                          />
                          <span className="min-w-0 flex-1 truncate" title={item.fileName}>
                            {item.fileName}
                          </span>
                          <span className="shrink-0 text-xs text-text-muted">
                            {t(STORAGE_REASON_KEYS[item.reason])}
                          </span>
                          <span className="w-20 shrink-0 text-right font-mono text-xs text-text-secondary">
                            {formatBytes(parseBytes(item.bytes))}
                          </span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </section>
            ) : null}

            {/* Artifacts kept for resumable tasks — read-only plus per-task abandon. */}
            {resumableGroups.length > 0 ? (
              <section aria-label={t("storageCenter.resumableTitle")} className="space-y-2">
                <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                  {t("storageCenter.resumableTitle", { bytes: formatBytes(totals.resumableBytes) })}
                </h2>
                <p className="text-xs text-text-muted">{t("storageCenter.resumableHint")}</p>
                {resumableGroups.map((task) => (
                  <div
                    key={task.taskId}
                    className="flex items-center gap-2 rounded-md border border-border-subtle bg-surface-base px-3 py-2 text-sm"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-text-primary" title={task.taskFileName}>
                        {task.taskFileName}
                      </p>
                      <p className="text-xs text-text-muted">
                        {task.protocol} · {task.items.length} · {formatBytes(task.totalBytes)}
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8 shrink-0 border-border-danger text-status-danger hover:bg-status-danger/10"
                      disabled={busy}
                      onClick={() => setConfirm({ kind: "task", task })}
                    >
                      <Trash2 className="h-3.5 w-3.5" aria-hidden />
                      {t("storageCenter.abandonResume")}
                    </Button>
                  </div>
                ))}
              </section>
            ) : null}

            <p className="pt-2 text-center text-xs text-text-muted">
              <button
                type="button"
                className="underline decoration-dotted hover:text-text-secondary"
                onClick={() => setNav("about")}
              >
                {t("storageCenter.aboutCleanup")}
              </button>
            </p>
          </div>
        )}
      </div>

      <ConfirmDialog
        confirm={confirm}
        selectedCount={selectedIds.size}
        selectedBytes={selectedBytes}
        busy={busy}
        onCancel={() => setConfirm(null)}
        onConfirm={executeConfirm}
      />
    </div>
  );
}

function ConfirmDialog({
  confirm,
  selectedCount,
  selectedBytes,
  busy,
  onCancel,
  onConfirm,
}: {
  confirm: ConfirmTarget | null;
  selectedCount: number;
  selectedBytes: number;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  if (!confirm) {
    return null;
  }
  const title =
    confirm.kind === "task"
      ? t("storageCenter.confirm.taskTitle")
      : t("storageCenter.confirm.title", { label: confirm.label });
  const description =
    confirm.kind === "task"
      ? t("storageCenter.confirm.taskDescription", {
          name: confirm.task.taskFileName,
          bytes: formatBytes(confirm.task.totalBytes),
        })
      : t("storageCenter.confirm.description", {
          count: confirm.kind === "mode" && confirm.mode === "selected" ? selectedCount : confirm.count,
          bytes: formatBytes(confirm.kind === "mode" && confirm.mode === "selected" ? selectedBytes : confirm.bytes),
        });
  return (
    <Dialog open onOpenChange={(open) => (!open ? onCancel() : undefined)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <DialogDescription className="text-sm text-text-secondary">{description}</DialogDescription>
          <p className="flex items-start gap-1.5 text-xs text-status-warning">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
            {t("storageCenter.confirm.permanentWarning")}
          </p>
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="ghost" className="w-full sm:w-auto" onClick={onCancel}>
            {t("storageCenter.confirm.cancel")}
          </Button>
          <Button type="button" variant="danger" className="w-full sm:w-auto" onClick={onConfirm} disabled={busy}>
            {busy ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : null}
            {t("storageCenter.confirm.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function scanHasItems(scan: StorageScanResult): boolean {
  return scan.items.length > 0;
}

function cnIcon(spinning: boolean): string {
  return spinning ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5";
}
