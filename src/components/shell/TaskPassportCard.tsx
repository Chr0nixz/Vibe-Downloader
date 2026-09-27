//! TaskPassportCard: the Integrity Passport summary for completed tasks
//! (feature proposal §2.4). Three outputs share one data source: the compact
//! card, the sanitized clipboard report, and the JSON export. Honesty rule:
//! `not_provided` checksum state renders explicit copy, never a green claim.

import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { IntegrityPassport } from "@/generated/bindings";
import { formatDateTime } from "@/lib/format-date";
import {
  buildPassportJson,
  buildPassportTextReport,
  bytesToNumber,
  PASSPORT_CHECKSUM_STATE_KEYS,
  PASSPORT_STAGING_KEYS,
  PASSPORT_VALIDATOR_KEYS,
  type PassportFileHash,
  splitRemoteValidators,
} from "@/lib/integrity-passport";
import { createLogger } from "@/lib/logger";
import { isTauriRuntime } from "@/lib/runtime";
import { computeFileHash, writeExportFile } from "@/lib/tauri";
import { cn, formatBytes } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

const log = createLogger("task-passport");

const CHECKSUM_STATE_TONE: Record<string, string> = {
  verified: "text-status-success",
  failed: "text-status-danger",
  pending: "text-status-warning",
  not_provided: "text-text-muted",
};

const STAGING_TONE: Record<string, string> = {
  complete: "text-status-success",
  incomplete: "text-status-warning",
  missing_output: "text-status-danger",
  not_applicable: "text-text-muted",
};

/** File-system-safe base name for the exported JSON. */
function sanitizeExportName(name: string): string {
  return name.replace(/[\\/:*?"<>|]+/g, "_").trim() || "task";
}

export function TaskPassportCard({
  task,
  passport,
  error,
}: {
  task: Task;
  passport: IntegrityPassport | null;
  error: string | null;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [computedHash, setComputedHash] = useState<PassportFileHash | null>(null);
  const [computing, setComputing] = useState(false);

  // Hooks stay above the early returns (rules of hooks); the passport is
  // only meaningful once the task completed — other statuses render nothing.
  if (task.status !== "completed") return null;

  const verifiedChecksum =
    passport?.checksums.find((checksum) => checksum.status === "verified" && checksum.actualHash) ?? null;
  const fileHash: PassportFileHash | null =
    verifiedChecksum && passport
      ? {
          algorithm: verifiedChecksum.algorithm,
          digest: verifiedChecksum.actualHash ?? "",
          source: "verified",
          computedAt: verifiedChecksum.verifiedAt ?? "",
        }
      : computedHash;

  async function handleCopyReport() {
    if (!passport) return;
    try {
      await navigator.clipboard?.writeText(buildPassportTextReport(passport, t));
      addToast({ tone: "success", title: t("taskDetails.passport.copyReportSuccess") });
    } catch {
      // UX-24: report the real clipboard outcome instead of assuming success.
      addToast({ tone: "error", title: t("taskDetails.passport.copyReportFailed") });
    }
  }

  async function handleExportJson() {
    if (!passport) return;
    const content = JSON.stringify(buildPassportJson(passport, fileHash), null, 2);
    const defaultPath = `integrity-passport-${sanitizeExportName(passport.fileName)}.json`;
    try {
      if (!isTauriRuntime()) {
        const blob = new Blob([content], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = defaultPath;
        document.body.appendChild(anchor);
        anchor.click();
        document.body.removeChild(anchor);
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      } else {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        await writeExportFile(path, content);
      }
      addToast({ tone: "success", title: t("taskDetails.passport.exportJsonSuccess") });
    } catch (err) {
      log.warn("passport JSON export failed", err);
      addToast({ tone: "error", title: t("taskDetails.passport.exportJsonFailed") });
    }
  }

  async function handleComputeHash() {
    if (!passport || computing) return;
    setComputing(true);
    try {
      const digest = await computeFileHash(task.id, "sha256");
      setComputedHash({
        algorithm: "sha256",
        digest,
        source: "computed_at_export",
        computedAt: new Date().toISOString(),
      });
      addToast({ tone: "success", title: t("taskDetails.passport.hashComputed") });
    } catch (err) {
      log.warn("passport hash computation failed", err);
      addToast({ tone: "error", title: t("taskDetails.passport.hashComputeFailed") });
    } finally {
      setComputing(false);
    }
  }

  if (error) {
    return (
      <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
        <p role="alert" className="text-[11px] text-status-danger">
          {t("taskDetails.passport.loadFailed")}
        </p>
      </div>
    );
  }
  if (!passport) return null;
  const { validators, rangeSupported } = splitRemoteValidators(passport.remoteValidators);

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <span className="font-semibold text-text-primary">{t("taskDetails.passport.title")}</span>
          <p className="mt-0.5 truncate text-[11px] text-text-muted">{t("taskDetails.passport.subtitle")}</p>
        </div>
      </div>

      <div className="mt-2 grid gap-1">
        <div className="flex items-center justify-between gap-2">
          <span className="text-text-muted">{t("taskDetails.passport.checksumsLabel")}</span>
          <span className={cn("font-medium", CHECKSUM_STATE_TONE[passport.checksumState])}>
            {t(PASSPORT_CHECKSUM_STATE_KEYS[passport.checksumState])}
          </span>
        </div>
        {passport.checksums.map((checksum) => (
          <div
            key={`${checksum.algorithm}-${checksum.verifiedAt ?? checksum.status}`}
            className="font-mono text-[11px] text-text-secondary"
          >
            <div className="truncate" title={checksum.actualHash ?? undefined}>
              <span className="text-text-muted">{checksum.algorithm.toUpperCase()} </span>
              {checksum.actualHash ?? checksum.errorMessage ?? "—"}
            </div>
          </div>
        ))}
        <div className="flex items-center justify-between gap-2">
          <span className="text-text-muted">{t("taskDetails.passport.validatorsLabel")}</span>
          <span className="text-right text-text-secondary">
            {validators.length > 0
              ? validators.map((validator) => t(PASSPORT_VALIDATOR_KEYS[validator])).join(", ")
              : t("taskDetails.passport.validatorsNone")}
          </span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <span className="text-text-muted">{t("taskDetails.passport.rangeLabel")}</span>
          <span className="text-right text-text-secondary">
            {t(rangeSupported ? "taskDetails.passport.rangeSupported" : "taskDetails.passport.rangeNotObserved")}
          </span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <span className="text-text-muted">{t("taskDetails.passport.stagingLabel")}</span>
          <span className={cn("font-medium", STAGING_TONE[passport.stagingCleanup])}>
            {t(PASSPORT_STAGING_KEYS[passport.stagingCleanup])}
          </span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <span className="text-text-muted">{t("taskDetails.passport.resumeStatsLabel")}</span>
          <span className="text-text-secondary">
            {t("taskDetails.passport.resumeStats", {
              resumes: passport.resumeCount,
              retries: passport.segmentRetries,
            })}
          </span>
        </div>
        {passport.totalBytes ? (
          <div className="flex items-center justify-between gap-2">
            <span className="text-text-muted">{t("taskDetails.passport.sizeLabel")}</span>
            <span className="font-mono text-text-secondary">{formatBytes(bytesToNumber(passport.totalBytes))}</span>
          </div>
        ) : (
          <div className="flex items-center justify-between gap-2">
            <span className="text-text-muted">{t("taskDetails.passport.sizeLabel")}</span>
            <span className="text-text-secondary">{t("taskDetails.passport.sizeUnknown")}</span>
          </div>
        )}
        {fileHash ? (
          <div className="font-mono text-[11px] text-text-secondary">
            <div className="truncate" title={fileHash.digest}>
              <span className="text-text-muted">
                {t("taskDetails.passport.fileHashLabel")} ({fileHash.algorithm.toUpperCase()}){" "}
              </span>
              {fileHash.digest}
            </div>
            <div className="text-[10px] text-text-muted">
              {fileHash.source === "verified"
                ? t("taskDetails.passport.fileHashVerified")
                : t("taskDetails.passport.fileHashComputed")}
              {fileHash.computedAt ? ` · ${formatDateTime(fileHash.computedAt, "dateTime")}` : ""}
            </div>
          </div>
        ) : null}
      </div>

      <div className="mt-2.5 flex flex-wrap justify-end gap-1.5">
        {!fileHash ? (
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-7 px-2 text-[11px]"
            disabled={computing}
            onClick={() => void handleComputeHash()}
          >
            {computing ? t("taskDetails.passport.computingHash") : t("taskDetails.passport.computeHash")}
          </Button>
        ) : null}
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-[11px]"
          onClick={() => void handleExportJson()}
        >
          {t("taskDetails.passport.exportJson")}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2 text-[11px]"
          onClick={() => void handleCopyReport()}
        >
          {t("taskDetails.passport.copyReport")}
        </Button>
      </div>
    </div>
  );
}
