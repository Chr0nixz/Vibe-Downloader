import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { useAppUpdater } from "@/hooks/use-app-updater";
import { formatSpeed } from "@/lib/utils";
import { useTaskStore } from "@/stores/task-store";

export function StatusBar() {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const { updateVersion, installing, error, installUpdate } = useAppUpdater();

  const stats = useMemo(() => {
    const active = tasks.filter(
      (t) => t.status === "downloading" || t.status === "retrying",
    );
    const queued = tasks.filter((t) => t.status === "queued");
    const totalSpeed = active.reduce((sum, t) => sum + t.speedBps, 0);
    return { active: active.length, queued: queued.length, totalSpeed };
  }, [tasks]);

  return (
    <footer className="flex h-9 shrink-0 items-center justify-between gap-3 border-t border-border-subtle bg-surface-base px-3 text-xs text-text-secondary md:h-8 md:px-4">
      <span className="min-w-0 truncate">
        {t("statusBar.total")}{" "}
        <span className="font-mono text-text-primary">
          {formatSpeed(stats.totalSpeed)}
        </span>
      </span>
      <span className="shrink-0">
        {t("statusBar.active")}{" "}
        <span className="font-mono text-text-primary">{stats.active}</span>
        <span className="hidden sm:inline">
          {" · "}
          {t("statusBar.queued")}{" "}
          <span className="font-mono text-text-primary">{stats.queued}</span>
        </span>
      </span>
      <span className="flex min-w-0 items-center justify-end gap-2">
        {updateVersion ? (
          <span className="flex items-center gap-2">
            <span className="truncate text-accent">
              {t("statusBar.updateAvailable", { version: updateVersion })}
            </span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-6 shrink-0 px-2 text-xs"
              disabled={installing}
              onClick={() => void installUpdate()}
            >
              {installing
                ? t("statusBar.updating")
                : t("statusBar.installUpdate")}
            </Button>
          </span>
        ) : error ? (
          <span className="truncate text-destructive" title={error}>
            {t("statusBar.updateFailed")}
          </span>
        ) : (
          <span className="hidden text-text-muted md:inline">
            {t("statusBar.noGlobalSpeedLimit")}
          </span>
        )}
      </span>
    </footer>
  );
}
