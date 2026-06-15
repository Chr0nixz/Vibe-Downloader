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

export function DeleteTaskDialog({
  task,
  open,
  onOpenChange,
  onDelete,
}: {
  task: Task | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDelete: (deleteFile: boolean) => void;
}) {
  const { t } = useTranslation();
  const [deleteFiles, setDeleteFiles] = useState(false);

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
          <DialogTitle>{t("deleteDialog.title")}</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <DialogDescription className="text-sm text-text-secondary">
            {task
              ? t("deleteDialog.messageWithName", { name: task.fileName })
              : t("deleteDialog.messageGeneric")}
          </DialogDescription>

          <label className="flex cursor-pointer items-center gap-2.5 rounded-md px-1 py-1 text-sm text-text-secondary transition-colors hover:text-text-primary">
            <input
              type="checkbox"
              checked={deleteFiles}
              onChange={(event) => setDeleteFiles(event.target.checked)}
              className="h-4 w-4 shrink-0 rounded border-border-subtle accent-accent-primary"
            />
            <span>{t("deleteDialog.alsoDeleteFiles")}</span>
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
            {t("deleteDialog.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
