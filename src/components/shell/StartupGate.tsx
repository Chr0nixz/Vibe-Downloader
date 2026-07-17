import { relaunch } from "@tauri-apps/plugin-process";
import { FolderOpen, RefreshCcw, ShieldAlert } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { StartupStatus } from "@/generated/bindings";
import { LOGO_64_DATA_URI } from "@/lib/logo";
import { getStartupStatus, openDatabaseRecoveryFolder, resetDatabaseForRecovery } from "@/lib/tauri";

export function StartupGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<StartupStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    // The backend runs heavy init (DB, migrations, settings, scheduler) on a
    // background task after showing the window. Poll get_startup_status until
    // it transitions out of "initializing" — otherwise the gate would freeze
    // on the first poll and never mount AppShell.
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const check = async () => {
      try {
        const next = await getStartupStatus();
        if (cancelled) return;
        setStatus(next);
        if (next.mode === "ready" || next.mode === "database_recovery_required") {
          return;
        }
      } catch (error) {
        if (cancelled) return;
        setLoadError(String(error));
        return;
      }
      if (!cancelled) timer = setTimeout(check, 300);
    };

    void check();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, []);

  if (status?.mode === "ready") return children;
  if (status?.mode === "database_recovery_required") return <DatabaseRecoveryPage status={status} />;

  // Match the inline HTML splash (logo + animated bar) so the transition
  // from index.html splash to React is seamless. The @keyframes and the
  // --vibe-splash-accent variable are defined inline in index.html and
  // remain available after React mounts.
  return (
    <main className="flex min-h-screen items-center justify-center bg-surface-root">
      <div role={loadError ? "alert" : "status"} className="flex flex-col items-center gap-5">
        <img
          src={LOGO_64_DATA_URI}
          alt=""
          style={{
            width: 56,
            height: 56,
            borderRadius: 14,
            animation: "vibe-splash-breathe 1.8s ease-in-out infinite",
          }}
        />
        {loadError ? (
          <p className="max-w-sm text-center text-sm font-medium text-status-danger">{loadError}</p>
        ) : (
          <div
            style={{
              width: 180,
              height: 2,
              borderRadius: 999,
              background: "var(--surface-track)",
              overflow: "hidden",
              position: "relative",
            }}
          >
            <div
              style={{
                position: "absolute",
                top: 0,
                left: "-40%",
                width: "40%",
                height: "100%",
                borderRadius: "inherit",
                background: "var(--vibe-splash-accent, oklch(0.4 0.18 235))",
                animation: "vibe-splash-slide 1.1s cubic-bezier(0.65, 0.05, 0.36, 1) infinite",
              }}
            />
          </div>
        )}
      </div>
    </main>
  );
}

function DatabaseRecoveryPage({ status }: { status: StartupStatus }) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState<"folder" | "retry" | "reset" | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run(action: "folder" | "retry" | "reset") {
    setBusy(action);
    setError(null);
    try {
      if (action === "folder") await openDatabaseRecoveryFolder();
      if (action === "retry") await relaunch();
      if (action === "reset") {
        await resetDatabaseForRecovery();
        await relaunch();
      }
    } catch (nextError) {
      setError(String(nextError));
      setBusy(null);
    }
  }

  return (
    <main className="flex min-h-screen items-center justify-center bg-surface-root px-4 py-8 text-text-primary sm:px-6">
      <section className="w-full max-w-2xl overflow-hidden rounded-xl border border-border-subtle bg-surface-base">
        <header className="flex gap-3 border-b border-border-divider px-5 py-5 sm:px-6">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-status-warning/15 text-status-warning">
            <ShieldAlert className="h-5 w-5" aria-hidden />
          </span>
          <div className="min-w-0">
            <h1 className="text-base font-semibold">{t("databaseRecovery.title")}</h1>
            <p className="mt-1 max-w-xl text-sm leading-6 text-text-secondary">{t("databaseRecovery.description")}</p>
          </div>
        </header>

        <div className="grid gap-4 px-5 py-5 sm:px-6">
          <p className={status.backupVerified ? "text-sm text-status-success" : "text-sm text-status-danger"}>
            {t(status.backupVerified ? "databaseRecovery.backupReady" : "databaseRecovery.backupUnavailable")}
          </p>
          <PathRow label={t("databaseRecovery.databasePath")} value={status.databasePath} />
          <PathRow label={t("databaseRecovery.backupPath")} value={status.backupPath} />
          {status.message ? <p className="text-xs leading-5 text-text-muted">{status.message}</p> : null}
          {error ? (
            <p role="alert" className="rounded-md bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
              {t("databaseRecovery.actionFailed", { error })}
            </p>
          ) : null}

          {confirming ? (
            <div className="rounded-lg bg-status-danger/10 px-4 py-3">
              <p className="text-sm font-semibold text-status-danger">{t("databaseRecovery.confirmTitle")}</p>
              <p className="mt-1 text-xs leading-5 text-text-secondary">{t("databaseRecovery.confirmDescription")}</p>
              <div className="mt-3 flex flex-wrap justify-end gap-2">
                <Button variant="ghost" disabled={busy !== null} onClick={() => setConfirming(false)}>
                  {t("databaseRecovery.cancel")}
                </Button>
                <Button variant="danger" disabled={!status.canReset || busy !== null} onClick={() => void run("reset")}>
                  {t("databaseRecovery.confirmRebuild")}
                </Button>
              </div>
            </div>
          ) : null}
        </div>

        <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-border-divider px-5 py-4 sm:px-6">
          <Button variant="ghost" disabled={busy !== null} onClick={() => void run("folder")}>
            <FolderOpen className="h-4 w-4" aria-hidden />
            {t("databaseRecovery.openFolder")}
          </Button>
          <Button variant="outline" disabled={busy !== null} onClick={() => void run("retry")}>
            <RefreshCcw className="h-4 w-4" aria-hidden />
            {t("databaseRecovery.retry")}
          </Button>
          <Button variant="danger" disabled={!status.canReset || busy !== null} onClick={() => setConfirming(true)}>
            {t("databaseRecovery.rebuild")}
          </Button>
        </footer>
      </section>
    </main>
  );
}

function PathRow({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="grid gap-1 sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-3">
      <span className="text-xs font-medium text-text-muted">{label}</span>
      <span dir="ltr" className="break-all font-mono text-xs leading-5 text-text-secondary">
        {value ?? "—"}
      </span>
    </div>
  );
}
