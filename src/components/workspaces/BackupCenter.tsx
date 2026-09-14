//! Backup & Migration Center (feature proposal §3.7): content inventory,
//! whole-file restore with pre-restore checks and migration remap, safe-subset
//! restore, and the post-restore reconfiguration report. Self-contained like
//! StorageCenter: fetches its own data, no task-store pagination. Paths shown
//! in this page come from backend results verbatim; the page never builds one.

import { Archive, CheckCircle2, DatabaseBackup, FolderInput, Loader2, ShieldAlert, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
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
  bytesToNumber,
  CONTENTS_FIELD_KEYS,
  CONTENTS_FIELD_ORDER,
  type DiskCheckLevel,
  diskCheckLevel,
  NOT_EXPORTED_ITEMS,
  NOT_EXPORTED_KEYS,
  reportEntries,
  SUBSET_FIELD_KEYS,
  SUBSET_FIELD_ORDER,
  SUBSET_HINT_KEYS,
} from "@/components/workspaces/backup-center-logic";
import type {
  BackupContents,
  BackupSubsetRestoreResult,
  BackupValidateResult,
  RestoreReport,
} from "@/generated/bindings";
import { exportAppBackup, pickBackupFile, pickRemapRoot } from "@/lib/backup";
import { errorMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import {
  describeBackupSource,
  dismissRestoreReport,
  getLastRestoreReport,
  restoreAppBackup,
  restoreBackupSubset,
  validateAppBackup,
} from "@/lib/tauri";
import { cn, formatBytes } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";

type SubsetSelection = { tasks: boolean; rules: boolean; settings: boolean };

const EMPTY_SELECTION: SubsetSelection = { tasks: false, rules: false, settings: false };

function cnIcon(busy: boolean): string {
  return cn("h-4 w-4", busy && "animate-spin");
}

export function BackupCenter() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  // Export section: live inventory + last export diagnostics.
  const [contents, setContents] = useState<BackupContents | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportResult, setExportResult] = useState<{
    path: string;
    usedCopyFallback: boolean;
  } | null>(null);

  // Whole restore section: validate result drives the check panel.
  const [validation, setValidation] = useState<BackupValidateResult | null>(null);
  const [validateBusy, setValidateBusy] = useState(false);
  const [remapEnabled, setRemapEnabled] = useState(false);
  const [remapRoot, setRemapRoot] = useState<string | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  // Subset restore section.
  const [subsetPath, setSubsetPath] = useState<string | null>(null);
  const [subsetSelection, setSubsetSelection] = useState<SubsetSelection>(EMPTY_SELECTION);
  const [subsetBusy, setSubsetBusy] = useState(false);

  // Post-restore report banner.
  const [report, setReport] = useState<RestoreReport | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextContents, nextReport] = await Promise.all([describeBackupSource(), getLastRestoreReport()]);
      setContents(nextContents);
      setReport(nextReport);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("backupCenter.toast.loadFailed"),
        description: errorMessage(err),
      });
    }
  }, [addToast, t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleExport = async () => {
    setExportBusy(true);
    try {
      const result = await exportAppBackup();
      if (result) {
        setExportResult({ path: result.path, usedCopyFallback: result.usedCopyFallback });
        addToast({ tone: "success", title: t("backupCenter.toast.exportSuccess") });
        void refresh();
      }
    } catch (err) {
      addToast({
        tone: "error",
        title: t("backupCenter.toast.exportFailed"),
        description: errorMessage(err),
      });
    } finally {
      setExportBusy(false);
    }
  };

  const handlePickForRestore = async () => {
    setValidateBusy(true);
    setValidation(null);
    setRemapEnabled(false);
    setRemapRoot(null);
    try {
      const path = await pickBackupFile("backupCenter.pickFileTitle");
      if (!path) return;
      const result = await validateAppBackup(path);
      setValidation(result);
      // §3.7: violations are the trigger for offering the migration remap.
      setRemapEnabled(result.pathPolicy.violationCount > 0);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("backupCenter.toast.validateFailed"),
        description: errorMessage(err),
      });
    } finally {
      setValidateBusy(false);
    }
  };

  const handlePickRemapRoot = async () => {
    const root = await pickRemapRoot("backupCenter.remap.pickTitle");
    if (root) setRemapRoot(root);
  };

  const handleRestore = async () => {
    if (!validation) return;
    setRestoreBusy(true);
    try {
      const result = await restoreAppBackup(validation.path, remapEnabled && remapRoot ? remapRoot : null);
      setConfirmOpen(false);
      addToast({
        tone: "success",
        title: t("backupCenter.toast.restoreSuccess"),
        description:
          result.remappedPaths > 0 ? t("backupCenter.remap.remappedPaths", { count: result.remappedPaths }) : undefined,
      });
      setValidation(null);
      setSubsetPath(null);
    } catch (err) {
      setConfirmOpen(false);
      addToast({
        tone: "error",
        title: t("backupCenter.toast.restoreFailed"),
        description: errorMessage(err),
      });
    } finally {
      setRestoreBusy(false);
    }
  };

  const handlePickForSubset = async () => {
    const path = await pickBackupFile("backupCenter.pickFileTitle");
    if (path) setSubsetPath(path);
  };

  const handleSubsetRestore = async () => {
    if (!subsetPath) return;
    setSubsetBusy(true);
    try {
      const result: BackupSubsetRestoreResult = await restoreBackupSubset(subsetPath, subsetSelection);
      addToast({
        tone: "success",
        title: t("backupCenter.toast.subsetSuccess"),
        description: t("backupCenter.subset.resultSummary", {
          tasks: result.tasksInserted,
          rules: result.rulesInserted,
          settings: result.settingsReplaced,
        }),
      });
      setSubsetSelection(EMPTY_SELECTION);
      setSubsetPath(null);
      void refresh();
    } catch (err) {
      addToast({
        tone: "error",
        title: t("backupCenter.toast.subsetFailed"),
        description: errorMessage(err),
      });
    } finally {
      setSubsetBusy(false);
    }
  };

  const handleDismissReport = async () => {
    try {
      await dismissRestoreReport();
      setReport(null);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("backupCenter.toast.dismissFailed"),
        description: errorMessage(err),
      });
    }
  };

  const entries = report ? reportEntries(report) : [];
  const disk: DiskCheckLevel | null = validation
    ? diskCheckLevel(validation.disk.freeBytes, validation.disk.requiredBytes)
    : null;
  const subsetDirty = subsetPath !== null && Object.values(subsetSelection).some(Boolean);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border-subtle px-3 py-2.5 md:px-4">
        <DatabaseBackup className="h-5 w-5 text-accent-primary" aria-hidden />
        <div className="min-w-0 flex-1">
          <h1 className="text-sm font-semibold text-text-primary">{t("backupCenter.title")}</h1>
          <p className="truncate text-xs text-text-muted">{t("backupCenter.subtitle")}</p>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3 md:px-4">
        <div className="space-y-5 pb-8">
          {report ? (
            <section
              aria-label={t("backupCenter.reportTitle")}
              className="space-y-2 rounded-md border border-status-warning/40 bg-status-warning/10 px-3 py-2.5"
            >
              <div className="flex flex-wrap items-center gap-2">
                <TriangleAlert className="h-4 w-4 shrink-0 text-status-warning" aria-hidden />
                <h2 className="text-sm font-semibold text-text-primary">{t("backupCenter.reportTitle")}</h2>
                {report.backupCreatedAt ? (
                  <span className="ml-auto text-xs text-text-muted">
                    {t("backupCenter.report.created", {
                      date: formatDateTime(report.backupCreatedAt, "dateTime"),
                    })}
                  </span>
                ) : null}
              </div>
              <ul className="space-y-1 text-sm text-text-secondary">
                {entries.map((entry) => (
                  <li key={entry.key} className="break-all">
                    {t(entry.key, entry.values)}
                  </li>
                ))}
              </ul>
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7"
                  onClick={() => void handleDismissReport()}
                >
                  {t("backupCenter.report.dismiss")}
                </Button>
              </div>
            </section>
          ) : null}

          {/* Export + inventory. */}
          <section aria-label={t("backupCenter.exportTitle")} className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {t("backupCenter.exportTitle")}
              </h2>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ml-auto h-8"
                onClick={() => void handleExport()}
                disabled={exportBusy}
              >
                {exportBusy ? (
                  <Loader2 className={cnIcon(true)} aria-hidden />
                ) : (
                  <Archive className="h-4 w-4" aria-hidden />
                )}
                {t("backupCenter.exportButton")}
              </Button>
            </div>
            {contents ? (
              <dl className="grid grid-cols-2 gap-x-4 gap-y-1 rounded-md border border-border-subtle bg-surface-base px-3 py-2 text-sm sm:grid-cols-3 lg:grid-cols-5">
                {CONTENTS_FIELD_ORDER.map((field) => (
                  <div key={field} className="flex items-baseline justify-between gap-2">
                    <dt className="text-xs text-text-muted">{t(CONTENTS_FIELD_KEYS[field])}</dt>
                    <dd className="font-medium text-text-primary">{contents[field]}</dd>
                  </div>
                ))}
              </dl>
            ) : (
              <div
                className="h-16 animate-pulse rounded-md border border-border-subtle bg-surface-base"
                role="status"
                aria-label={t("backupCenter.loading")}
              />
            )}
            {exportResult ? (
              <p className="break-all text-xs text-text-muted">
                {t("backupCenter.exportResultPath", { path: exportResult.path })}
                {exportResult.usedCopyFallback ? (
                  <span className="ml-2 text-status-warning">{t("backupCenter.usedCopyFallback")}</span>
                ) : null}
              </p>
            ) : null}
            <div className="rounded-md border border-border-subtle bg-surface-base px-3 py-2 text-sm">
              <p className="flex items-center gap-1.5 text-xs font-medium text-text-secondary">
                <ShieldAlert className="h-3.5 w-3.5 text-status-warning" aria-hidden />
                {t("backupCenter.notExportedTitle")}
              </p>
              <ul className="mt-1 list-inside list-disc space-y-0.5 text-xs text-text-muted">
                {NOT_EXPORTED_ITEMS.map((item) => (
                  <li key={item}>{t(NOT_EXPORTED_KEYS[item])}</li>
                ))}
              </ul>
            </div>
          </section>

          {/* Whole restore. */}
          <section aria-label={t("backupCenter.restoreTitle")} className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {t("backupCenter.restoreTitle")}
              </h2>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ml-auto h-8"
                onClick={() => void handlePickForRestore()}
                disabled={validateBusy}
              >
                {validateBusy ? (
                  <Loader2 className={cnIcon(true)} aria-hidden />
                ) : (
                  <FolderInput className="h-4 w-4" aria-hidden />
                )}
                {t("backupCenter.pickFile")}
              </Button>
            </div>
            {validation ? (
              <div className="space-y-2 rounded-md border border-border-subtle bg-surface-base px-3 py-2.5 text-sm">
                <dl className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-text-muted">
                  <span>{t("backupCenter.manifest.appVersion", { version: validation.appVersion })}</span>
                  <span>
                    {t("backupCenter.manifest.schemaVersion", {
                      version: validation.schemaVersion,
                    })}
                  </span>
                  <span>
                    {t("backupCenter.manifest.databaseBytes", {
                      bytes: formatBytes(bytesToNumber(validation.databaseBytes)),
                    })}
                  </span>
                  <span>{formatDateTime(validation.createdAt, "dateTime")}</span>
                </dl>
                <RestoreChecks validation={validation} disk={disk ?? "unknown"} />
                <div className="space-y-1.5 border-t border-border-subtle pt-2">
                  <label className="flex items-center gap-2 text-sm text-text-secondary">
                    <input
                      type="checkbox"
                      className="h-4 w-4"
                      checked={remapEnabled}
                      onChange={(event) => setRemapEnabled(event.target.checked)}
                    />
                    {t("backupCenter.remap.toggle")}
                  </label>
                  <p className="text-xs text-text-muted">{t("backupCenter.remap.description")}</p>
                  {remapEnabled ? (
                    <div className="flex flex-wrap items-center gap-2">
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7"
                        onClick={() => void handlePickRemapRoot()}
                      >
                        {t("backupCenter.remap.pick")}
                      </Button>
                      {remapRoot ? (
                        <span className="break-all font-mono text-xs text-text-secondary" title={remapRoot}>
                          {remapRoot}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                </div>
                <div className="flex justify-end border-t border-border-subtle pt-2">
                  <Button
                    type="button"
                    variant="danger"
                    size="sm"
                    className="h-8"
                    onClick={() => setConfirmOpen(true)}
                    disabled={remapEnabled && !remapRoot}
                  >
                    {t("backupCenter.confirm.open")}
                  </Button>
                </div>
              </div>
            ) : null}
          </section>

          {/* Subset restore. */}
          <section aria-label={t("backupCenter.subsetTitle")} className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {t("backupCenter.subsetTitle")}
              </h2>
              <span className="max-w-full truncate font-mono text-xs text-text-muted" title={subsetPath ?? undefined}>
                {subsetPath ?? ""}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ml-auto h-8"
                onClick={() => void handlePickForSubset()}
              >
                <FolderInput className="h-4 w-4" aria-hidden />
                {t("backupCenter.pickFile")}
              </Button>
            </div>
            <div className="space-y-1.5 rounded-md border border-border-subtle bg-surface-base px-3 py-2.5 text-sm">
              <p className="text-xs text-text-muted">{t("backupCenter.subsetDescription")}</p>
              {SUBSET_FIELD_ORDER.map((field) => (
                <label key={field} className="flex items-start gap-2 text-sm text-text-secondary">
                  <input
                    type="checkbox"
                    className="mt-0.5 h-4 w-4"
                    checked={subsetSelection[field]}
                    onChange={(event) =>
                      setSubsetSelection((previous) => ({
                        ...previous,
                        [field]: event.target.checked,
                      }))
                    }
                  />
                  <span>
                    {t(SUBSET_FIELD_KEYS[field])}
                    <span className="block text-xs text-text-muted">{t(SUBSET_HINT_KEYS[field])}</span>
                  </span>
                </label>
              ))}
              <div className="flex justify-end pt-1">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8"
                  onClick={() => void handleSubsetRestore()}
                  disabled={!subsetDirty || subsetBusy}
                >
                  {subsetBusy ? <Loader2 className={cnIcon(true)} aria-hidden /> : null}
                  {t("backupCenter.subset.run")}
                </Button>
              </div>
            </div>
          </section>
        </div>
      </div>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("backupCenter.confirm.title")}</DialogTitle>
            <DialogDescription>{t("backupCenter.confirm.description")}</DialogDescription>
          </DialogHeader>
          <DialogBody>
            <p className="flex items-start gap-2 rounded-md bg-status-warning/10 px-3 py-2 text-sm text-status-warning">
              <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
              {t("backupCenter.confirm.warning")}
            </p>
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="ghost" size="sm" onClick={() => setConfirmOpen(false)}>
              {t("backupCenter.confirm.cancel")}
            </Button>
            <Button
              type="button"
              variant="danger"
              size="sm"
              onClick={() => void handleRestore()}
              disabled={restoreBusy}
            >
              {restoreBusy ? (
                <Loader2 className={cnIcon(true)} aria-hidden />
              ) : (
                <CheckCircle2 className="h-4 w-4" aria-hidden />
              )}
              {t("backupCenter.confirm.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function RestoreChecks({ validation, disk }: { validation: BackupValidateResult; disk: DiskCheckLevel }) {
  const { t } = useTranslation();
  const { pathPolicy, settingsPreview, disk: diskCheck } = validation;
  return (
    <div className="space-y-1 text-xs">
      <p className="flex items-center gap-1.5 text-text-secondary">
        {disk === "low" ? (
          <TriangleAlert className="h-3.5 w-3.5 text-status-danger" aria-hidden />
        ) : (
          <CheckCircle2 className="h-3.5 w-3.5 text-status-success" aria-hidden />
        )}
        {t("backupCenter.check.disk", {
          free: diskCheck.freeBytes
            ? formatBytes(bytesToNumber(diskCheck.freeBytes))
            : t("backupCenter.check.diskUnknownValue"),
          required: formatBytes(bytesToNumber(diskCheck.requiredBytes)),
        })}
      </p>
      {pathPolicy.violationCount === 0 ? (
        <p className="flex items-center gap-1.5 text-text-secondary">
          <CheckCircle2 className="h-3.5 w-3.5 text-status-success" aria-hidden />
          {t("backupCenter.check.pathsOk")}
        </p>
      ) : (
        <div className="space-y-0.5 text-status-warning">
          <p className="flex items-center gap-1.5">
            <TriangleAlert className="h-3.5 w-3.5" aria-hidden />
            {t("backupCenter.check.pathsWarning", { count: pathPolicy.violationCount })}
          </p>
          {pathPolicy.offendingSaveDirs.length > 0 ? (
            <p className="break-all text-text-muted">
              {t("backupCenter.check.offendingDirs", {
                dirs: pathPolicy.offendingSaveDirs.join(", "),
              })}
            </p>
          ) : null}
        </div>
      )}
      {settingsPreview.ffmpegConfigured ? (
        <p className="text-text-muted">{t("backupCenter.check.scrubFfmpeg")}</p>
      ) : null}
      {settingsPreview.completionAction !== "notify" ? (
        <p className="text-text-muted">
          {t("backupCenter.check.scrubCompletion", {
            action: settingsPreview.completionAction,
          })}
        </p>
      ) : null}
      {settingsPreview.proxyPasswordSaved ? (
        <p className="text-text-muted">{t("backupCenter.check.scrubProxy")}</p>
      ) : null}
      {settingsPreview.defaultSaveDir ? (
        <p className="break-all text-text-muted">
          {t("backupCenter.check.defaultSaveDir", { dir: settingsPreview.defaultSaveDir })}
        </p>
      ) : null}
    </div>
  );
}
