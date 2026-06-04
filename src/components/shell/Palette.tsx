import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { seedMockTasks } from "@/lib/tauri";
import { useTaskStore } from "@/stores/task-store";

export function Palette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const setTasks = useTaskStore((s) => s.setTasks);
  const setError = useTaskStore((s) => s.setError);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Command palette</DialogTitle>
        </DialogHeader>
        <button
          type="button"
          className="mt-2 w-full rounded-md bg-surface-raised px-3 py-2 text-left text-sm"
          onClick={async () => {
            try {
              setTasks(await seedMockTasks());
              setError(null);
            } catch (e) {
              setError(String(e));
            }
            onOpenChange(false);
          }}
        >
          Reset mock tasks
        </button>
      </DialogContent>
    </Dialog>
  );
}
