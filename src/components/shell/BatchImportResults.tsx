import { Clipboard, Download, RotateCcw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { BatchImportResult } from "@/generated/bindings";
import { parseAppError } from "@/lib/errors";
import { isTauriRuntime } from "@/lib/runtime";
import { writeExportFile } from "@/lib/tauri";
import { sanitizeUrlForDisplay } from "@/lib/utils";

type Item = BatchImportResult["items"][number];
export function isFailedBatchItem(item: Item): boolean {
  return !item.task && !item.duplicate && (!item.valid || Boolean(item.errorMessage));
}

export function BatchImportResults({
  result,
  busy,
  onRetry,
}: {
  result: BatchImportResult;
  busy: boolean;
  onRetry?: (indices: number[], urls: string[]) => void;
}) {
  const { t } = useTranslation();
  const [edits, setEdits] = useState<Record<number, string>>({});
  const [feedback, setFeedback] = useState<{ error: boolean; text: string } | null>(null);
  const rows = result.items.map((item, index) => ({ item, index }));
  const failed = rows.filter(({ item }) => isFailedBatchItem(item));
  const others = rows.filter(({ item }) => !isFailedBatchItem(item));
  const urlFor = (index: number) => edits[index] ?? result.items[index].inputUrl;
  const retry = (indices: number[]) => onRetry?.(indices, indices.map(urlFor));

  async function copyFailed() {
    try {
      if (!navigator.clipboard) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(failed.map(({ index }) => urlFor(index)).join("\n"));
      setFeedback({ error: false, text: t("newDownload.batchCopied") });
    } catch {
      setFeedback({ error: true, text: t("newDownload.batchCopyFailed") });
    }
  }

  async function exportResult() {
    // Reports contain only recovery fields; task objects include local paths
    // and opaque errors may contain authentication material from a server.
    const content = JSON.stringify(
      rows.map(({ item, index }) => ({
        line: index + 1,
        url: sanitizeUrlForDisplay(urlFor(index)),
        status: item.task ? "created" : item.duplicate ? "duplicate" : isFailedBatchItem(item) ? "failed" : "ready",
        errorCode: item.errorMessage ? (parseAppError(item.errorMessage)?.code ?? null) : null,
      })),
      null,
      2,
    );
    try {
      if (isTauriRuntime()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath: "vibe-batch-result.json",
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        await writeExportFile(path, content);
      } else {
        const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = "vibe-batch-result.json";
        document.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      setFeedback({ error: false, text: t("newDownload.batchExported") });
    } catch {
      setFeedback({ error: true, text: t("newDownload.batchExportFailed") });
    }
  }

  function renderRow({ item, index }: { item: Item; index: number }) {
    const failedItem = isFailedBatchItem(item);
    return (
      <div key={index} className="grid min-w-0 gap-1 border-b border-border-divider py-2" data-batch-line={index + 1}>
        <span className="truncate font-mono text-text-primary" title={sanitizeUrlForDisplay(item.inputUrl)}>
          {item.fileName ?? sanitizeUrlForDisplay(item.normalizedUrl ?? item.inputUrl)}
        </span>
        <span className={`break-words ${failedItem ? "text-status-danger" : "text-text-muted"}`}>
          {item.errorMessage ?? t(item.task ? "newDownload.batchCreated" : "newDownload.batchReady")}
        </span>
        {failedItem && onRetry && (
          <div className="flex min-w-0 items-center gap-2">
            <Input
              value={edits[index] ?? sanitizeUrlForDisplay(item.inputUrl)}
              aria-label={t("newDownload.batchEditUrl", { line: index + 1 })}
              onChange={(event) => setEdits((values) => ({ ...values, [index]: event.target.value }))}
              disabled={busy}
              className="h-8 min-w-0 flex-1 font-mono text-xs"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy || !urlFor(index).trim()}
              onClick={() => retry([index])}
              aria-label={t("newDownload.batchRetryOne", { line: index + 1 })}
            >
              <RotateCcw className="h-3.5 w-3.5" />
              {t("actions.retry")}
            </Button>
          </div>
        )}
      </div>
    );
  }

  return (
    <section className="grid min-w-0 gap-2 text-xs" aria-label={t("newDownload.batchResults")}>
      <p role="status" className="text-text-secondary">
        {t("newDownload.batchSummary", {
          total: result.items.length,
          created: result.createdCount,
          failed: result.failedCount,
          duplicate: result.duplicateCount,
        })}
      </p>
      <div className="flex flex-wrap gap-2">
        {failed.length > 0 && (
          <>
            {onRetry && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={busy || failed.some(({ index }) => !urlFor(index).trim())}
                onClick={() => retry(failed.map(({ index }) => index))}
              >
                <RotateCcw className="h-3.5 w-3.5" />
                {t("newDownload.batchRetryFailed")}
              </Button>
            )}
            <Button type="button" variant="outline" size="sm" onClick={() => void copyFailed()}>
              <Clipboard className="h-3.5 w-3.5" />
              {t("newDownload.batchCopyFailedUrls")}
            </Button>
          </>
        )}
        <Button type="button" variant="ghost" size="sm" onClick={() => void exportResult()}>
          <Download className="h-3.5 w-3.5" />
          {t("taskList.exportJson")}
        </Button>
      </div>
      {feedback && (
        <p
          role={feedback.error ? "alert" : "status"}
          className={feedback.error ? "text-status-danger" : "text-text-secondary"}
        >
          {feedback.text}
        </p>
      )}
      <div className="max-h-72 overflow-auto pr-1">
        {failed.map(renderRow)}
        {others.length > 0 && (
          <details open={failed.length === 0}>
            <summary className="cursor-pointer py-2 text-text-muted">
              {t("newDownload.batchOtherResults", { total: others.length })}
            </summary>
            {others.map(renderRow)}
          </details>
        )}
      </div>
    </section>
  );
}
