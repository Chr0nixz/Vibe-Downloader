import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
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
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete task</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 p-4 text-sm">
          <p className="text-text-secondary">
            {task
              ? `Delete "${task.fileName}" from the task list?`
              : "Delete this task from the task list?"}
          </p>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="button" variant="outline" onClick={() => onDelete(false)}>
              Delete record
            </Button>
            <Button type="button" variant="danger" onClick={() => onDelete(true)}>
              Delete files too
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
