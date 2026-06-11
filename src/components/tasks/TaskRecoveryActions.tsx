import {
  Clock,
  FilePenLine,
  FolderOpen,
  HardDrive,
  Link,
  RotateCcw,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { RecoveryAction } from "@/generated/bindings";
import { errorMessage, recoveryActionsForError } from "@/lib/errors";
import type { Task } from "@/types/task";

export function TaskRecoveryActions({
  task,
  onResolve,
  compact = false,
}: {
  task: Task;
  onResolve: (task: Task, action: RecoveryAction) => void;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const actions =
    task.recoveryActions.length > 0
      ? task.recoveryActions
      : recoveryActionsForError(task.errorMessage);
  if (!task.errorMessage || actions.length === 0) return null;

  return (
    <div className="rounded-md border border-status-danger/25 bg-status-danger/10 px-3 py-2">
      <p className="text-xs leading-5 text-status-danger">
        {errorMessage(task.errorMessage)}
      </p>
      <div className="mt-2 flex flex-wrap gap-2">
        {actions.map((action) => (
          <Button
            key={action}
            type="button"
            variant={action === "restart" ? "danger" : "outline"}
            className={compact ? "h-8" : "h-9"}
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
    </div>
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
    case "retry_later":
      return <Clock className="h-4 w-4" aria-hidden />;
    case "restart":
    case "retry":
    default:
      return <RotateCcw className="h-4 w-4" aria-hidden />;
  }
}
