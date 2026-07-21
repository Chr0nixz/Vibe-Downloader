import { Check, ClipboardCopy, LoaderCircle, RefreshCw, Wrench } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type {
  EnvironmentFixAction,
  EnvironmentFixKind,
  EnvironmentHealthItem,
  EnvironmentHealthReport,
  EnvironmentHealthStatus,
} from "@/generated/bindings";
import { useAppUpdater } from "@/hooks/use-app-updater";
import { exportAppBackup } from "@/lib/backup";
import { formatEnvironmentReport } from "@/lib/environment-report";
import { createLogger } from "@/lib/logger";
import { getEnvironmentHealth, runEnvironmentFix } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";

const log = createLogger("environment-panel");

type EnvironmentPanelProps = {
  onFocusSection: (sectionId: string) => void;
};

export function EnvironmentPanel({ onFocusSection }: EnvironmentPanelProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const updater = useAppUpdater();
  const [report, setReport] = useState<EnvironmentHealthReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [fixingKey, setFixingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const runCheck = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await getEnvironmentHealth();
      setReport(next);
    } catch (err) {
      log.warn("environment health check failed", err);
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      addToast({
        title: t("settings.environmentCheckFailed"),
        description: message,
        tone: "error",
      });
    } finally {
      setLoading(false);
    }
  }, [addToast, t]);

  useEffect(() => {
    void runCheck();
  }, [runCheck]);

  const overall = useMemo(() => summarizeStatuses(report?.items ?? []), [report]);

  async function copyReport() {
    if (!report) return;
    const text = formatEnvironmentReport(report, {
      currentVersion: updater.currentVersion,
      updateVersion: updater.updateVersion,
      status: updater.status,
      error: updater.error,
    });
    try {
      await navigator.clipboard.writeText(text);
      addToast({
        title: t("settings.environmentReportCopied"),
        tone: "success",
      });
    } catch (err) {
      log.warn("copy environment report failed", err);
      addToast({
        title: t("settings.environmentReportCopyFailed"),
        tone: "error",
      });
    }
  }

  async function applyFix(item: EnvironmentHealthItem, action: EnvironmentFixAction) {
    const key = `${item.id}:${action.kind}:${action.browser ?? ""}:${action.pathKind ?? ""}`;
    setFixingKey(key);
    try {
      if (action.kind === "export_backup") {
        const result = await exportAppBackup();
        if (result) {
          addToast({
            title: t("settings.dataBackupExportSuccess", { path: result.path }),
            tone: "success",
          });
        }
        onFocusSection("data-backup");
        return;
      }
      if (action.kind === "check_for_update") {
        await updater.checkForUpdate();
        onFocusSection("about-updates");
        return;
      }

      const result = await runEnvironmentFix({
        kind: action.kind,
        browser: action.browser,
        pathKind: action.pathKind,
        section: action.section,
      });
      addToast({
        title: result.ok ? t("settings.environmentFixApplied") : t("settings.environmentFixFailed"),
        description: result.message,
        tone: result.ok ? "success" : "error",
      });
      if (result.focusSection) {
        onFocusSection(result.focusSection);
      }
      if (result.refresh) {
        await runCheck();
      }
    } catch (err) {
      log.warn("environment fix failed", err);
      addToast({
        title: t("settings.environmentFixFailed"),
        description: err instanceof Error ? err.message : String(err),
        tone: "error",
      });
    } finally {
      setFixingKey(null);
    }
  }

  async function fixAllSafe() {
    if (!report) return;
    const batchInstall = report.items
      .flatMap((item) => item.suggestedActions.map((action) => ({ item, action })))
      .find(({ action }) => action.kind === "install_native_host" && action.browser == null);
    if (!batchInstall) {
      addToast({
        title: t("settings.environmentNoSafeFixes"),
        tone: "info",
      });
      return;
    }
    await applyFix(batchInstall.item, batchInstall.action);
  }

  const updaterItem = {
    status: updaterStatusToHealth(updater.status),
    summary: updaterSummary(updater, t),
    detail: updater.error,
  };

  return (
    <div className="grid gap-0" data-search-key="environment_health">
      <div className="flex flex-wrap items-center gap-2 border-b border-border-divider px-4 py-3">
        <Button
          type="button"
          variant="outline"
          className="h-11 md:h-8"
          disabled={loading}
          onClick={() => void runCheck()}
        >
          {loading ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
          {t("settings.environmentRunCheck")}
        </Button>
        <Button
          type="button"
          variant="outline"
          className="h-11 md:h-8"
          disabled={!report || loading}
          onClick={() => void copyReport()}
        >
          <ClipboardCopy className="h-4 w-4" />
          {t("settings.environmentCopyReport")}
        </Button>
        <Button
          type="button"
          variant="ghost"
          className="h-11 md:h-8"
          disabled={!report || loading || fixingKey !== null}
          onClick={() => void fixAllSafe()}
        >
          <Wrench className="h-4 w-4" />
          {t("settings.environmentFixSafe")}
        </Button>
        {report ? (
          <span className="ml-auto text-xs text-text-muted">
            {t("settings.environmentSummary", {
              ok: overall.ok,
              warn: overall.warn,
              error: overall.error,
            })}
          </span>
        ) : null}
      </div>

      {error ? (
        <p className="px-4 py-3 text-sm text-status-danger" role="alert">
          {error}
        </p>
      ) : null}

      {loading && !report ? (
        <div className="flex items-center gap-2 px-4 py-4 text-sm text-text-muted">
          <LoaderCircle className="h-4 w-4 animate-spin" />
          {t("settings.environmentChecking")}
        </div>
      ) : null}

      {report ? (
        <div className="grid">
          {report.items.map((item) => (
            <HealthRow
              key={item.id}
              item={item}
              title={itemTitle(item.id, t)}
              fixingKey={fixingKey}
              onFix={(action) => void applyFix(item, action)}
              fixLabel={(kind, browser) => fixActionLabel(kind, browser, t)}
            />
          ))}
          <div className="grid gap-2 border-t border-border-divider px-4 py-3 md:grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)_auto] md:items-start">
            <div className="flex items-center gap-2 text-sm font-medium text-text-secondary">
              <StatusDot status={updaterItem.status} />
              {t("settings.environmentItemUpdater")}
            </div>
            <div className="min-w-0">
              <p className="text-sm text-text-primary">{updaterItem.summary}</p>
              {updaterItem.detail ? (
                <p className="mt-1 truncate text-xs text-text-muted" title={updaterItem.detail}>
                  {updaterItem.detail}
                </p>
              ) : null}
            </div>
            <Button
              type="button"
              variant="outline"
              className="h-11 justify-center md:h-8"
              disabled={!updater.isTauri || updater.checking || updater.installing}
              onClick={() => void updater.checkForUpdate()}
            >
              {updater.checking ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
              {t("settings.checkForUpdates")}
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function HealthRow({
  item,
  title,
  fixingKey,
  onFix,
  fixLabel,
}: {
  item: EnvironmentHealthItem;
  title: string;
  fixingKey: string | null;
  onFix: (action: EnvironmentFixAction) => void;
  fixLabel: (kind: EnvironmentFixKind, browser: string | null) => string;
}) {
  const primary =
    item.suggestedActions.find((action) => action.kind === "install_native_host" && action.browser == null) ??
    item.suggestedActions[0] ??
    null;
  const key = primary ? `${item.id}:${primary.kind}:${primary.browser ?? ""}:${primary.pathKind ?? ""}` : null;

  return (
    <div className="grid gap-2 border-t border-border-divider px-4 py-3 first:border-t-0 md:grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)_auto] md:items-start">
      <div className="flex items-center gap-2 text-sm font-medium text-text-secondary">
        <StatusDot status={item.status} />
        <span>{title}</span>
      </div>
      <div className="min-w-0">
        <p className="text-sm text-text-primary">{item.summary}</p>
        {item.detail ? (
          <p className="mt-1 truncate text-xs text-text-muted" title={item.detail}>
            {item.detail}
          </p>
        ) : null}
      </div>
      {primary ? (
        <Button
          type="button"
          variant="outline"
          className="h-11 justify-center md:h-8"
          disabled={fixingKey !== null}
          onClick={() => onFix(primary)}
        >
          {fixingKey === key ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Wrench className="h-4 w-4" />}
          {fixLabel(primary.kind, primary.browser)}
        </Button>
      ) : (
        <span className="inline-flex h-8 items-center justify-center text-xs text-status-success md:justify-end">
          <Check className="mr-1 h-3.5 w-3.5" />
        </span>
      )}
    </div>
  );
}

function StatusDot({ status }: { status: EnvironmentHealthStatus }) {
  return (
    <span
      className={cn(
        "inline-block h-2.5 w-2.5 shrink-0 rounded-full",
        status === "ok" && "bg-status-success",
        status === "warn" && "bg-status-warning",
        status === "error" && "bg-status-danger",
        status === "unknown" && "bg-text-muted",
      )}
      aria-hidden
    />
  );
}

function summarizeStatuses(items: EnvironmentHealthItem[]) {
  let ok = 0;
  let warn = 0;
  let error = 0;
  for (const item of items) {
    if (item.status === "ok") ok += 1;
    else if (item.status === "warn") warn += 1;
    else if (item.status === "error") error += 1;
  }
  return { ok, warn, error };
}

function updaterStatusToHealth(status: ReturnType<typeof useAppUpdater>["status"]): EnvironmentHealthStatus {
  switch (status) {
    case "up-to-date":
      return "ok";
    case "available":
    case "downloading":
    case "installing":
      return "warn";
    case "error":
      return "error";
    default:
      return "unknown";
  }
}

function updaterSummary(
  updater: ReturnType<typeof useAppUpdater>,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (updater.status) {
    case "up-to-date":
      return t("settings.upToDate");
    case "available":
      return t("settings.updateAvailableTitle", { version: updater.updateVersion ?? "" });
    case "checking":
      return t("settings.checkingForUpdates");
    case "downloading":
      return t("settings.downloadingUpdate");
    case "installing":
      return t("settings.installingUpdate");
    case "error":
      return t("settings.updateCheckFailed");
    default:
      return t("settings.checkForUpdates");
  }
}

function itemTitle(id: string, t: (key: string) => string): string {
  switch (id) {
    case "native_host":
      return t("settings.environmentItemNativeHost");
    case "browser":
      return t("settings.environmentItemBrowser");
    case "ffmpeg":
      return t("settings.environmentItemFfmpeg");
    case "proxy":
      return t("settings.environmentItemProxy");
    case "save_dir":
      return t("settings.environmentItemSaveDir");
    case "disk":
      return t("settings.environmentItemDisk");
    case "database":
      return t("settings.environmentItemDatabase");
    default:
      return id;
  }
}

function fixActionLabel(
  kind: EnvironmentFixKind,
  browser: string | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (kind) {
    case "install_native_host":
      return browser
        ? t("settings.environmentFixInstallBrowser", { browser })
        : t("settings.environmentFixInstallNativeHost");
    case "open_path":
      return t("settings.environmentFixOpenPath");
    case "focus_setting":
      return t("settings.environmentFixOpenSetting");
    case "export_backup":
      return t("settings.dataBackupExport");
    case "check_for_update":
      return t("settings.checkForUpdates");
    default:
      return t("settings.environmentFixOpenSetting");
  }
}
