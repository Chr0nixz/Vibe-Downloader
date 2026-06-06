import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("deleteDialog.title")}</DialogTitle>
        </DialogHeader>
        <DialogBody className="py-4 text-sm">
          <p className="text-text-secondary">
            {task
              ? t("deleteDialog.messageWithName", { name: task.fileName })
              : t("deleteDialog.messageGeneric")}
          </p>
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
            variant="outline"
            className="w-full sm:w-auto"
            onClick={() => onDelete(false)}
          >
            {t("deleteDialog.deleteRecord")}
          </Button>
          <Button
            type="button"
            variant="danger"
            className="w-full sm:w-auto"
            onClick={() => onDelete(true)}
          >
            {t("deleteDialog.deleteFilesToo")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
