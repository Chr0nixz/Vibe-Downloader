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

/**
 * Hard confirm for deleting a task together with its downloaded files.
 *
 * Metadata-only removal is handled by an undoable soft-delete flow (see
 * AppShell `softDelete`); this dialog is only shown when the user explicitly
 * chooses to delete files from disk, which is irreversible and therefore
 * still warrants a confirm.
 */
export function DeleteTaskDialog({
  task,
  open,
  onOpenChange,
  onDelete,
}: {
  task: Task | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("deleteDialog.filesTitle")}</DialogTitle>
        </DialogHeader>
        <DialogBody className="py-4">
          <DialogDescription className="text-sm text-text-secondary">
            {task
              ? t("deleteDialog.filesMessageWithName", { name: task.fileName })
              : t("deleteDialog.filesMessageGeneric")}
          </DialogDescription>
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="ghost" className="w-full sm:w-auto" onClick={() => onOpenChange(false)}>
            {t("deleteDialog.cancel")}
          </Button>
          <Button type="button" variant="danger" className="w-full sm:w-auto" onClick={onDelete}>
            {t("deleteDialog.filesConfirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
