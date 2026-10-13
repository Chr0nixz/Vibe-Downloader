import { useEffect, useRef, useState } from "react";
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
import { Input } from "@/components/ui/input";
import type { BulkTaskOptionsPatch, BulkTaskOptionsResult, TaskPriority } from "@/generated/bindings";
import { runBulkOptions } from "@/lib/workflow-library";
import { useTaskDataStore } from "@/stores/task-data-store";
import { useTaskUIStore } from "@/stores/task-ui-store";

export function BulkTaskOptionsDialog({
  ids,
  onClose,
  onComplete,
}: {
  ids: string[];
  onClose: () => void;
  onComplete: () => void;
}) {
  const { t } = useTranslation();
  const [targets] = useState(() => {
    const deleting = new Set(useTaskUIStore.getState().pendingDeleteIds);
    return [...new Set(ids)].filter((id) => !deleting.has(id));
  });
  const [priority, setPriority] = useState("keep");
  const [speedAction, setSpeedAction] = useState("keep");
  const [speed, setSpeed] = useState("");
  const [categoryAction, setCategoryAction] = useState("keep");
  const [category, setCategory] = useState("");
  const [schedule, setSchedule] = useState("keep");
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<BulkTaskOptionsResult[]>([]);
  const [error, setError] = useState(false);
  const cancelled = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      cancelled.current = true;
    };
  }, []);
  const patch: BulkTaskOptionsPatch = {
    priority: priority === "keep" ? null : (priority as TaskPriority),
    taskSpeedLimitBps: speedAction === "set" ? speed.trim() : null,
    clearSpeedLimit: speedAction === "clear",
    categoryKey: categoryAction === "set" ? category.trim() : null,
    clearCategory: categoryAction === "clear",
    obeySchedule: schedule === "keep" ? null : schedule === "on",
  };
  const changed = priority !== "keep" || speedAction !== "keep" || categoryAction !== "keep" || schedule !== "keep";
  const speedValid = /^[1-9]\d{0,15}$/.test(speed.trim()) && Number.isSafeInteger(Number(speed.trim()));
  const categoryValid =
    Boolean(category.trim()) &&
    new TextEncoder().encode(category.trim()).length <= 256 &&
    !Array.from(category).some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127);
  const valid =
    targets.length > 0 &&
    changed &&
    (speedAction !== "set" || speedValid) &&
    (categoryAction !== "set" || categoryValid);
  const retryIds = results
    .filter((row) => row.status === "failed" || row.status === "not_processed")
    .map((row) => row.id);
  const [common] = useState(() => {
    const tasks = targets.map((id) => useTaskDataStore.getState().taskById[id]).filter((task) => task !== undefined);
    const value = (field: "priority" | "taskSpeedLimitBps" | "categoryKey" | "obeySchedule") => {
      const values = new Set(tasks.map((task) => String(task[field] ?? "")));
      return values.size === 1 ? [...values][0] : null;
    };
    return {
      priority: value("priority"),
      taskSpeedLimitBps: value("taskSpeedLimitBps"),
      categoryKey: value("categoryKey"),
      obeySchedule: value("obeySchedule"),
    };
  });
  async function apply(targetIds: string[]) {
    if (!valid) {
      setError(true);
      return;
    }
    cancelled.current = false;
    setBusy(true);
    setError(false);
    const prior = results.filter((row) => !targetIds.includes(row.id));
    try {
      await runBulkOptions(targetIds, patch, {
        isCancelled: () => cancelled.current,
        onProgress: (next) => {
          if (mounted.current) setResults([...prior, ...next]);
        },
      });
    } finally {
      if (mounted.current) {
        setBusy(false);
        onComplete();
      }
    }
  }
  function field(
    label: string,
    value: string,
    change: (value: string) => void,
    options: [string, string][],
    current: string | null | undefined,
  ) {
    return (
      <label className="flex flex-col gap-1 text-xs text-text-muted">
        {label}
        <span>
          {t("workflow.current")}: {current === null ? t("workflow.mixed") : current || t("workflow.default")}
        </span>
        <select
          aria-label={label}
          className="h-8 rounded-md border border-border-subtle bg-surface-root px-2 text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
          value={value}
          disabled={busy || results.length > 0}
          onChange={(event) => change(event.target.value)}
        >
          <option value="keep">{t("workflow.keep")}</option>
          {options.map(([key, text]) => (
            <option key={key} value={key}>
              {text}
            </option>
          ))}
        </select>
      </label>
    );
  }
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("workflow.bulkEdit")}</DialogTitle>
          <DialogDescription>{t("workflow.bulkHint", { total: targets.length })}</DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-3">
          {field(
            t("taskDetails.priority"),
            priority,
            setPriority,
            [
              ["low", t("queueCenter.priority.low")],
              ["normal", t("queueCenter.priority.normal")],
              ["high", t("queueCenter.priority.high")],
            ],
            common.priority
              ? t(`queueCenter.priority.${common.priority}` as "queueCenter.priority.normal")
              : common.priority,
          )}
          {field(
            t("taskDetails.taskSpeedLimit"),
            speedAction,
            setSpeedAction,
            [
              ["set", t("workflow.set")],
              ["clear", t("workflow.clear")],
            ],
            common.taskSpeedLimitBps,
          )}
          {speedAction === "set" ? (
            <label htmlFor="bulk-options-speed" className="flex flex-col gap-1 text-xs text-text-muted">
              {t("workflow.bytesPerSecond")}
              <Input
                id="bulk-options-speed"
                inputMode="numeric"
                maxLength={16}
                value={speed}
                aria-invalid={!speedValid}
                aria-describedby={!speedValid ? "bulk-options-speed-error" : undefined}
                disabled={busy || results.length > 0}
                onChange={(event) => setSpeed(event.target.value)}
              />
              {!speedValid ? (
                <span id="bulk-options-speed-error" className="text-status-danger">
                  {t("workflow.invalidSpeed")}
                </span>
              ) : null}
            </label>
          ) : null}
          {field(
            t("newDownload.category"),
            categoryAction,
            setCategoryAction,
            [
              ["set", t("workflow.set")],
              ["clear", t("workflow.clear")],
            ],
            common.categoryKey,
          )}
          {categoryAction === "set" ? (
            <label htmlFor="bulk-options-category" className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.category")}
              <Input
                id="bulk-options-category"
                value={category}
                maxLength={256}
                aria-invalid={!categoryValid}
                aria-describedby={!categoryValid ? "bulk-options-category-error" : undefined}
                disabled={busy || results.length > 0}
                onChange={(event) => setCategory(event.target.value)}
              />
              {!categoryValid ? (
                <span id="bulk-options-category-error" className="text-status-danger">
                  {t("workflow.invalidCategory")}
                </span>
              ) : null}
            </label>
          ) : null}
          {field(
            t("newDownload.obeySchedule"),
            schedule,
            setSchedule,
            [
              ["on", t("workflow.on")],
              ["off", t("workflow.off")],
            ],
            common.obeySchedule === null ? null : t(common.obeySchedule === "true" ? "workflow.on" : "workflow.off"),
          )}
          {error ? (
            <p role="alert" className="text-xs text-status-danger">
              {t("workflow.error")}
            </p>
          ) : null}
          {results.length > 0 ? (
            <div className="space-y-2 text-xs">
              <p role="status">
                {t("workflow.bulkResult", {
                  applied: results.filter((r) => r.status === "applied").length,
                  skipped: results.filter((r) => r.status === "skipped").length,
                  failed: results.filter((r) => r.status === "failed").length,
                  pending: results.filter((r) => r.status === "not_processed").length,
                })}
              </p>
              <ul className="max-h-40 space-y-1 overflow-auto">
                {results
                  .filter((r) => r.status !== "applied")
                  .map((row) => (
                    <li key={row.id} className="break-words">
                      {useTaskDataStore.getState().taskById[row.id]?.fileName ?? row.id} ·{" "}
                      {t(
                        row.status === "skipped"
                          ? "workflow.skipped"
                          : row.status === "not_processed"
                            ? "workflow.pending"
                            : "workflow.failed",
                      )}
                    </li>
                  ))}
              </ul>
            </div>
          ) : null}
        </DialogBody>
        <DialogFooter className="flex flex-wrap gap-2">
          {busy ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                cancelled.current = true;
              }}
            >
              {t("workflow.stopBatch")}
            </Button>
          ) : (
            <Button type="button" variant="outline" onClick={onClose}>
              {t("workflow.close")}
            </Button>
          )}
          <Button
            type="button"
            disabled={busy || !valid || (results.length > 0 && retryIds.length === 0)}
            onClick={() => void apply(results.length > 0 ? retryIds : targets)}
          >
            {t(results.length > 0 ? "workflow.retry" : "workflow.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
