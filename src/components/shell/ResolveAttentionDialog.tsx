import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { restartCost } from "@/components/tasks/TaskRecoveryActions";
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
import type { RecoveryAction } from "@/generated/bindings";
import { formatBytes } from "@/lib/utils";
import type { Task } from "@/types/task";

export type AttentionDialogAction = Extract<RecoveryAction, "choose_another_name" | "restart"> | "pause";

export interface AttentionDialogRequest {
  task: Task;
  action: AttentionDialogAction;
}

export function ResolveAttentionDialog({
  request,
  open,
  onOpenChange,
  onResolve,
}: {
  request: AttentionDialogRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onResolve: (fileName?: string) => void;
}) {
  const { t } = useTranslation();
  const [fileName, setFileName] = useState("");
  const isSaveAs = request?.action === "choose_another_name";
  const isPause = request?.action === "pause";

  useEffect(() => {
    setFileName(request?.task.fileName ?? "");
  }, [request]);

  if (!request) return null;
  const cost = isSaveAs
    ? null
    : isPause
      ? request.task.downloadedBytes > 0
        ? request.task.totalSize > 0
          ? t("recovery.pauseCost", {
              downloaded: formatBytes(request.task.downloadedBytes),
              total: formatBytes(request.task.totalSize),
            })
          : t("recovery.pauseCostUnknownTotal", { downloaded: formatBytes(request.task.downloadedBytes) })
        : null
      : restartCost(request.task, t);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (isSaveAs) {
      const nextName = fileName.trim();
      if (!nextName) return;
      onResolve(nextName);
      return;
    }
    onResolve();
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* Restart destroys downloaded bytes, so it interrupts as an alert
          dialog; Save as is an ordinary form. */}
      <DialogContent role={isSaveAs ? undefined : "alertdialog"}>
        <DialogHeader>
          <DialogTitle>
            {isSaveAs
              ? t("recoveryDialog.saveAsTitle")
              : isPause
                ? t("recoveryDialog.pauseTitle")
                : t("recoveryDialog.restartTitle")}
          </DialogTitle>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
          <DialogBody className="space-y-4 py-4">
            {/* Restart names the object, then the consequence once: the generic
                "will discard the current progress" line used to precede the
                byte-exact cost that says the same thing more precisely. */}
            {!isSaveAs ? (
              <p className="truncate text-sm font-medium text-text-primary" title={request.task.fileName}>
                {request.task.fileName}
              </p>
            ) : null}
            <DialogDescription className={cost ? "leading-5 text-text-secondary" : undefined}>
              {isSaveAs
                ? t("recoveryDialog.saveAsDescription", {
                    name: request.task.fileName,
                  })
                : isPause
                  ? (cost ??
                    t("recoveryDialog.pauseDescription", {
                      name: request.task.fileName,
                    }))
                  : (cost ??
                    t("recoveryDialog.restartDescription", {
                      name: request.task.fileName,
                    }))}
            </DialogDescription>
            {isSaveAs ? (
              <label htmlFor="recovery-file-name" className="flex flex-col gap-1 text-xs text-text-muted">
                {t("recoveryDialog.fileName")}
                <Input
                  id="recovery-file-name"
                  value={fileName}
                  onChange={(event) => setFileName(event.target.value)}
                  className="h-11 md:h-8"
                  autoFocus
                  required
                />
              </label>
            ) : null}
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="ghost" className="w-full sm:w-auto" onClick={() => onOpenChange(false)}>
              {t("recoveryDialog.cancel")}
            </Button>
            <Button
              type="submit"
              variant={isSaveAs ? "default" : "danger"}
              className="w-full sm:w-auto"
              disabled={isSaveAs && !fileName.trim()}
            >
              {isSaveAs
                ? t("recoveryDialog.confirmSaveAs")
                : isPause
                  ? t("recoveryDialog.confirmPause")
                  : t("recoveryDialog.confirmRestart")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
