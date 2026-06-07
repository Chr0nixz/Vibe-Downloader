import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { seedMockTasks } from "@/lib/tauri";
import { useTaskStore } from "@/stores/task-store";

export function Palette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const setTasks = useTaskStore((s) => s.setTasks);
  const setError = useTaskStore((s) => s.setError);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("palette.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("palette.description")}
          </DialogDescription>
        </DialogHeader>
        <DialogBody className="py-4">
          <Button
            type="button"
            variant="outline"
            className="w-full justify-start"
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
            {t("palette.resetMockTasks")}
          </Button>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
