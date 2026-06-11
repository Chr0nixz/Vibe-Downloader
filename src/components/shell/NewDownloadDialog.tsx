import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen } from "lucide-react";

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
import { Input } from "@/components/ui/input";
import type { BatchImportResult, ProbeTaskPayload } from "@/generated/bindings";
import { createLogger } from "@/lib/logger";
import { errorMessage } from "@/lib/errors";
import { createTask, importUrls, openDirectoryPicker, probeTask } from "@/lib/tauri";

const log = createLogger("new-download");
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { parseByteCount } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { Task } from "@/types/task";

export function NewDownloadDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (task: Task) => void;
}) {
  const { t } = useTranslation();
  const settings = useSettingsStore((s) => s.settings);
  const [url, setUrl] = useState("");
  const [saveDir, setSaveDir] = useState("");
  const [fileName, setFileName] = useState("");
  const [expectedHashSha256, setExpectedHashSha256] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<ProbeTaskPayload | null>(null);
  const [probeUrl, setProbeUrl] = useState("");
  const [batchInput, setBatchInput] = useState("");
  const [batchResult, setBatchResult] = useState<BatchImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [submitStatus, setSubmitStatus] = useState<string | null>(null);
  const probeRequestId = useRef(0);
  const isTorrentProbe = probe?.protocol === "bt" || probe?.protocol === "magnet";

  async function detect(nextUrl = url.trim(), automatic = false) {
    if (!nextUrl) return;
    const requestId = ++probeRequestId.current;
    setProbing(true);
    if (!automatic) setError(null);
    setProbe(null);
    setProbeUrl("");
    try {
      const nextProbe = await probeTask({ url: nextUrl });
      if (requestId !== probeRequestId.current) return;
      setProbe(nextProbe);
      setProbeUrl(nextUrl);
      if (!fileName.trim()) {
        setFileName(nextProbe.fileName);
      }
      setError(null);
    } catch (err) {
      if (requestId !== probeRequestId.current) return;
      log.warn("probe failed", err);
      setError(errorMessage(err));
    } finally {
      if (requestId === probeRequestId.current) setProbing(false);
    }
  }

  useEffect(() => {
    const nextUrl = url.trim();
    setSubmitStatus(null);
    if (!nextUrl) {
      setProbe(null);
      setProbeUrl("");
      setError(null);
      return;
    }

    const timeoutId = window.setTimeout(() => {
      void detect(nextUrl, true);
    }, 650);

    return () => window.clearTimeout(timeoutId);
  }, [url]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    setSubmitStatus(
      probe && probeUrl === url.trim()
        ? t("newDownload.usingProbe")
        : t("newDownload.revalidating"),
    );
    try {
      const task = await createTask({
        url: url.trim(),
        saveDir: saveDir.trim() || null,
        fileName: fileName.trim() || null,
        expectedHashSha256: isTorrentProbe ? null : expectedHashSha256.trim() || null,
      });
      onCreated(task);
      setUrl("");
      setSaveDir("");
      setFileName("");
      setExpectedHashSha256("");
      setProbe(null);
      setProbeUrl("");
      setBatchResult(null);
      setSubmitStatus(null);
      onOpenChange(false);
    } catch (err) {
      log.error("create task failed", err);
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
      setSubmitStatus(null);
    }
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setSaveDir(selected);
  }

  async function runBatch(create: boolean) {
    if (!batchInput.trim()) return;
    setSubmitting(create);
    setError(null);
    try {
      const result = await importUrls({
        input: batchInput,
        saveDir: saveDir.trim() || null,
        probe: true,
        create,
      });
      setBatchResult(result);
      for (const item of result.items) {
        if (item.task) onCreated(normalizeTask(item.task));
      }
      if (create && result.createdCount > 0) {
        setBatchInput("");
      }
    } catch (err) {
      log.error("batch import failed", err);
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("newDownload.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("newDownload.description")}
          </DialogDescription>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col overflow-hidden" onSubmit={submit}>
          <DialogBody className="flex flex-col gap-3 py-4">
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.url")}
              <Input
                value={url}
                onChange={(event) => {
                  setUrl(event.target.value);
                  setProbe(null);
                  setProbeUrl("");
                }}
                placeholder={t("newDownload.urlPlaceholder")}
                className="h-11 md:h-8"
                autoFocus
                required
              />
            </label>
            <div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-11 md:h-8"
                onClick={() => void detect()}
                title={t("newDownload.detectHint")}
                disabled={probing || !url.trim()}
              >
                {probing ? t("newDownload.detecting") : t("newDownload.detect")}
              </Button>
            </div>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.saveDir")}
              <div className="flex flex-col gap-2 md:flex-row">
                <Input
                  value={saveDir}
                  onChange={(event) => setSaveDir(event.target.value)}
                  placeholder={
                    settings?.defaultSaveDir ?? t("newDownload.saveDirPlaceholder")
                  }
                  className="h-11 md:h-8"
                />
                <Button
                  type="button"
                  variant="outline"
                  className="h-11 shrink-0 md:h-8"
                  onClick={chooseDirectory}
                  disabled={submitting}
                >
                  <FolderOpen className="h-4 w-4" />
                  {t("newDownload.chooseDirectory")}
                </Button>
              </div>
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.fileName")}
              <Input
                value={fileName}
                onChange={(event) => setFileName(event.target.value)}
                placeholder={t("newDownload.fileNamePlaceholder")}
                className="h-11 md:h-8"
              />
            </label>
            {!isTorrentProbe ? (
              <label className="flex flex-col gap-1 text-xs text-text-muted">
                {t("newDownload.sha256")}
                <Input
                  value={expectedHashSha256}
                  onChange={(event) => setExpectedHashSha256(event.target.value)}
                  placeholder={t("newDownload.sha256Placeholder")}
                  className="h-11 font-mono md:h-8"
                />
              </label>
            ) : null}
            {probe ? (
              <div className="grid grid-cols-1 gap-2 rounded-md border border-border-subtle bg-surface-raised/60 p-3 text-xs sm:grid-cols-2">
                <Info label={t("newDownload.probeFile")} value={probe.fileName} />
                <Info label={t("newDownload.probeSize")} value={formatBytes(parseByteCount(probe.totalSize))} />
                <Info label={t("newDownload.probeHost")} value={probe.sourceKey} />
                <Info
                  label={t("newDownload.probeResume")}
                  value={probe.capabilities.supportsResume ? t("newDownload.resumeSupported") : t("newDownload.resumeUnavailable")}
                />
              </div>
            ) : null}
            <div className="grid gap-2 rounded-md border border-border-subtle bg-surface-root/40 p-3">
              <label className="flex flex-col gap-1 text-xs text-text-muted">
                {t("newDownload.batchUrls")}
                <textarea
                  value={batchInput}
                  onChange={(event) => setBatchInput(event.target.value)}
                  placeholder={t("newDownload.batchUrlsPlaceholder")}
                  className="min-h-24 resize-y rounded-md border border-border-subtle bg-surface-base px-3 py-2 font-mono text-xs text-text-primary outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                />
              </label>
              <div className="flex flex-col gap-2 sm:flex-row">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-9"
                  onClick={() => void runBatch(false)}
                  disabled={submitting || !batchInput.trim()}
                >
                  {t("newDownload.previewBatch")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-9"
                  onClick={() => void runBatch(true)}
                  disabled={submitting || !batchInput.trim()}
                >
                  {t("newDownload.createBatch")}
                </Button>
              </div>
              {batchResult ? (
                <BatchResultSummary result={batchResult} />
              ) : null}
            </div>
            {!probe && probing ? (
              <p className="rounded-md border border-border-subtle bg-surface-raised/60 px-3 py-2 text-xs text-text-secondary">
                {t("newDownload.autoDetecting")}
              </p>
            ) : null}
            {submitStatus ? (
              <p role="status" className="rounded-md border border-border-accent bg-accent-primary/10 px-3 py-2 text-xs text-accent-primary">
                {submitStatus}
              </p>
            ) : null}
            {error ? (
              <div role="alert" className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
                <p>{error}</p>
                <p className="mt-1 text-text-secondary">
                  {t("newDownload.probeFailedHint")}
                </p>
              </div>
            ) : null}
          </DialogBody>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              className="w-full sm:w-auto"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("newDownload.cancel")}
            </Button>
            <Button type="submit" className="w-full sm:w-auto" disabled={submitting}>
              {submitting ? (
                <>
                  <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                  {t("newDownload.starting")}
                </>
              ) : t("newDownload.start")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function BatchResultSummary({ result }: { result: BatchImportResult }) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-2 text-xs">
      <p className="text-text-secondary">
        {t("newDownload.batchSummary", {
          total: result.items.length,
          created: result.createdCount,
          failed: result.failedCount,
          duplicate: result.duplicateCount,
        })}
      </p>
      {result.items.slice(0, 5).map((item) => (
        <div
          key={`${item.inputUrl}-${item.normalizedUrl ?? "invalid"}`}
          className="grid gap-1 rounded-md border border-border-divider bg-surface-raised/50 px-2 py-1"
        >
          <span className="truncate font-mono text-text-primary">
            {item.fileName ?? sanitizeUrlForDisplay(item.normalizedUrl ?? item.inputUrl)}
          </span>
          <span className={item.valid ? "text-text-muted" : "text-status-danger"}>
            {item.errorMessage ?? (item.task ? t("newDownload.batchCreated") : t("newDownload.batchReady"))}
          </span>
        </div>
      ))}
    </div>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-text-muted">{label}</div>
      <div className="truncate text-text-primary">{value}</div>
    </div>
  );
}
