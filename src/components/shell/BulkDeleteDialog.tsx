import { useState } from "react";
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
import type { Task } from "@/types/task";

const MAX_VISIBLE_NAMES = 6;

export function BulkDeleteDialog({
  tasks,
  open,
  onOpenChange,
  onDelete,
}: {
  tasks: Task[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDelete: (deleteFile: boolean) => void;
}) {
  const { t } = useTranslation();
  const [deleteFiles, setDeleteFiles] = useState(false);

  const visibleNames = tasks.slice(0, MAX_VISIBLE_NAMES);
  const remainingCount = tasks.length - MAX_VISIBLE_NAMES;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) setDeleteFiles(false);
        onOpenChange(nextOpen);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("deleteDialog.bulkTitle", { count: tasks.length })}
          </DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <DialogDescription className="text-sm text-text-secondary">
            {t("deleteDialog.bulkDescription", { count: tasks.length })}
          </DialogDescription>

          {tasks.length > 0 ? (
            <ul
              className="max-h-36 space-y-0.5 overflow-y-auto overscroll-contain rounded-md border border-border-subtle bg-surface-root px-3 py-2"
              role="list"
            >
              {visibleNames.map((task) => (
                <li
                  key={task.id}
                  className="flex items-center gap-2 py-0.5 text-sm"
                >
                  <span className="truncate text-text-primary">
                    {task.fileName}
                  </span>
                </li>
              ))}
              {remainingCount > 0 ? (
                <li className="py-0.5 text-xs text-text-muted">
                  {t("deleteDialog.bulkMore", { count: remainingCount })}
                </li>
              ) : null}
            </ul>
          ) : null}

          <label className="flex cursor-pointer items-center gap-2.5 rounded-md px-1 py-1 text-sm text-text-secondary transition-colors hover:text-text-primary">
            <input
              type="checkbox"
              checked={deleteFiles}
              onChange={(event) => setDeleteFiles(event.target.checked)}
              className="h-4 w-4 shrink-0 rounded border-border-subtle accent-accent-primary"
            />
            <span>{t("deleteDialog.bulkDeleteFiles")}</span>
          </label>
        </DialogBody>
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            className="w-full sm:w-auto"
            onClick={() => onOpenChange(false)}
          >
            {t("deleteDialog.cancel")}
          </Button>
          <Button
            type="button"
            variant="danger"
            className="w-full sm:w-auto"
            onClick={() => onDelete(deleteFiles)}
          >
            {t("deleteDialog.bulkConfirm", { count: tasks.length })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
