import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { clearSettingsRecoveryReturn, readSettingsRecoveryReturn } from "@/lib/settings-recovery-return";
import { resolveTaskAttention, retryTask } from "@/lib/tauri";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";

export function SettingsRecoveryReturnBanner({ ffmpegReady }: { ffmpegReady: boolean }) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [returnState] = useState(() => readSettingsRecoveryReturn());
  const task = useTaskDataStore((s) => (returnState ? (s.taskById[returnState.taskId] ?? null) : null));
  const setNav = useTaskUIStore((s) => s.setNav);
  const selectTask = useTaskUIStore((s) => s.selectTask);

  useEffect(() => {
    if (returnState && !task) clearSettingsRecoveryReturn();
  }, [returnState, task]);

  const goBack = useCallback(
    async (retry: boolean) => {
      if (!task || !returnState) return;
      clearSettingsRecoveryReturn();
      const dest = task.status === "needs_attention" ? "attention" : "all";
      setNav(dest);
      selectTask(task.id);
      if (!retry) return;
      try {
        if (task.status === "needs_attention") {
          await resolveTaskAttention({ id: task.id, action: "retry", fileName: null, saveDir: null });
        } else {
          await retryTask(task.id);
        }
      } catch (error) {
        addToast({
          tone: "error",
          title: t("toast.actionFailed"),
          description: error instanceof Error ? error.message : String(error),
        });
      }
    },
    [addToast, returnState, selectTask, setNav, t, task],
  );

  if (!returnState || !task) return null;

  const retryEnabled = returnState.action === "manage_sftp_host_keys" || ffmpegReady;

  return (
    <div
      className="mb-4 flex flex-wrap items-center gap-2 rounded-md border border-border-accent-subtle bg-accent-primary/8 px-3 py-2 text-sm text-text-primary"
      role="status"
    >
      <p className="min-w-0 flex-1">{t("recovery.configuringFor", { name: task.fileName })}</p>
      <div className="flex shrink-0 flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" onClick={() => void goBack(false)}>
          {t("recovery.returnToTask")}
        </Button>
        <Button type="button" size="sm" disabled={!retryEnabled} onClick={() => void goBack(true)}>
          {t("actions.retry")}
        </Button>
      </div>
    </div>
  );
}
