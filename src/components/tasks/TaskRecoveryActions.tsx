import { Clock, Copy, FilePenLine, FolderOpen, HardDrive, Link, RotateCcw, Wrench } from "lucide-react";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { RecoveryAction } from "@/generated/bindings";
import { formatErrorForReport, localizedErrorMessage, recoveryActionsForError } from "@/lib/errors";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

export function TaskRecoveryActions({
  task,
  onResolve,
}: {
  task: Task;
  onResolve: (task: Task, action: RecoveryAction) => void;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const recoveryActions = task.recoveryActions ?? [];
  const actions = recoveryActions.length > 0 ? recoveryActions : recoveryActionsForError(task.errorMessage);

  const handleCopy = useCallback(() => {
    if (!task.errorMessage) return;
    const text = formatErrorForReport(task.errorMessage, t, { taskId: task.id, url: task.url });
    navigator.clipboard.writeText(text).catch(() => {});
    addToast({
      tone: "info",
      title: t("recovery.errorCopied"),
    });
  }, [addToast, task.errorMessage, task.id, task.url, t]);

  if (!task.errorMessage || actions.length === 0) return null;

  return (
    <fieldset className="m-0 min-w-0 rounded-md border border-border-danger-subtle bg-status-danger/10 px-3 py-2">
      <legend className="sr-only">{t("recovery.groupLabel")}</legend>
      <div className="flex items-start justify-between gap-2">
        <p role="alert" className="text-xs leading-5 text-status-danger">
          {localizedErrorMessage(task.errorMessage, t)}
        </p>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="shrink-0 px-1.5"
          aria-label={t("recovery.copyError")}
          title={t("recovery.copyError")}
          onClick={(event) => {
            event.stopPropagation();
            handleCopy();
          }}
        >
          <Copy className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
      <div className="mt-2 flex flex-wrap gap-2">
        {actions.map((action) => (
          <Button
            key={action}
            type="button"
            variant={action === "restart" ? "danger" : "outline"}
            className="h-8"
            onClick={(event) => {
              event.stopPropagation();
              onResolve(task, action);
            }}
          >
            <RecoveryIcon action={action} />
            {t(`recovery.${action}`)}
          </Button>
        ))}
      </div>
    </fieldset>
  );
}

function RecoveryIcon({ action }: { action: RecoveryAction }) {
  switch (action) {
    case "choose_another_name":
      return <FilePenLine className="h-4 w-4" aria-hidden />;
    case "choose_another_folder":
    case "open_folder":
      return <FolderOpen className="h-4 w-4" aria-hidden />;
    case "free_disk_space":
      return <HardDrive className="h-4 w-4" aria-hidden />;
    case "check_url":
      return <Link className="h-4 w-4" aria-hidden />;
    case "configure_ffmpeg":
      return <Wrench className="h-4 w-4" aria-hidden />;
    case "retry_later":
      return <Clock className="h-4 w-4" aria-hidden />;
    default:
      return <RotateCcw className="h-4 w-4" aria-hidden />;
  }
}
