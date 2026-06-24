import { type FormEvent, useEffect, useState } from "react";
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
import type { RecoveryAction } from "@/generated/bindings";
import type { Task } from "@/types/task";

export type AttentionDialogAction = Extract<RecoveryAction, "choose_another_name" | "restart">;

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

  useEffect(() => {
    setFileName(request?.task.fileName ?? "");
  }, [request]);

  if (!request) return null;

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
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{isSaveAs ? t("recoveryDialog.saveAsTitle") : t("recoveryDialog.restartTitle")}</DialogTitle>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
          <DialogBody className="space-y-4 py-4">
            <DialogDescription>
              {isSaveAs
                ? t("recoveryDialog.saveAsDescription", {
                    name: request.task.fileName,
                  })
                : t("recoveryDialog.restartDescription", {
                    name: request.task.fileName,
                  })}
            </DialogDescription>
            {isSaveAs ? (
              <label className="flex flex-col gap-1 text-xs text-text-muted">
                {t("recoveryDialog.fileName")}
                <Input
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
              {isSaveAs ? t("recoveryDialog.confirmSaveAs") : t("recoveryDialog.confirmRestart")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
