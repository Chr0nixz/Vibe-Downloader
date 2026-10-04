import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CloseRequestAction, CloseRequestPayload } from "@/generated/bindings";

interface CloseDownloadDialogProps {
  request: CloseRequestPayload | null;
  open: boolean;
  onCancel: () => void;
  onAction: (action: Exclude<CloseRequestAction, "cancel">, remember: boolean) => Promise<void>;
}

/** Offers an explicit close policy while active download owners still exist. */
export function CloseDownloadDialog({ request, open, onCancel, onAction }: CloseDownloadDialogProps) {
  const { t } = useTranslation();
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) {
      setRemember(false);
      setBusy(false);
    }
  }, [open]);

  if (!request) return null;

  const submit = async (action: Exclude<CloseRequestAction, "cancel">) => {
    setBusy(true);
    try {
      await onAction(action, remember);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !busy) onCancel();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("closeDialog.title")}</DialogTitle>
        </DialogHeader>
        <DialogBody className="space-y-4 py-4">
          <DialogDescription>
            {request.statsUnavailable
              ? t("closeDialog.statsUnavailable")
              : t("closeDialog.description", {
                  active: request.active,
                  queued: request.queued > 0 ? t("closeDialog.queuedSuffix", { count: request.queued }) : "",
                })}
          </DialogDescription>
          <label htmlFor="close-dialog-remember" className="flex items-start gap-3 text-sm text-text-secondary">
            <Checkbox
              id="close-dialog-remember"
              checked={remember}
              disabled={busy}
              onChange={(event) => setRemember(event.currentTarget.checked)}
              aria-label={t("closeDialog.remember")}
            />
            <span>{t("closeDialog.remember")}</span>
          </label>
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onCancel}>
            {t("closeDialog.cancel")}
          </Button>
          <Button variant="outline" disabled={busy} onClick={() => void submit("pause_exit")}>
            {t("closeDialog.pauseExit")}
          </Button>
          <Button disabled={busy} onClick={() => void submit("tray")}>
            {t("closeDialog.tray")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
