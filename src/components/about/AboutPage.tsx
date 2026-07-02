import {
  BookOpen,
  Bug,
  Check,
  Clipboard,
  Code,
  ExternalLink,
  HelpCircle,
  LoaderCircle,
  RotateCcw,
  Sparkles,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { MenuItem, MenuSeparator, RegionContextMenu } from "@/components/ui/menu-item";
import { useAppUpdater } from "@/hooks/use-app-updater";
import { cn, formatBytes } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";

const REPO_URL = "https://github.com/Chr0nixz/Vibe-Downloader";
const LINKS = [
  { key: "linksGithub", href: REPO_URL, icon: Code },
  { key: "linksDocs", href: `${REPO_URL}#readme`, icon: BookOpen },
  { key: "linksIssues", href: `${REPO_URL}/issues`, icon: Bug },
] as const;

export function AboutPage({ onOpenOnboarding }: { onOpenOnboarding: () => void }) {
  const { t } = useTranslation();
  const updater = useAppUpdater();
  const addToast = useToastStore((s) => s.addToast);
  const version = updater.currentVersion ?? "0.1.1";

  const copyVersion = async () => {
    try {
      await navigator.clipboard?.writeText(version);
      addToast({ tone: "success", title: t("contextmenu.about.versionCopied") });
    } catch {
      addToast({ tone: "error", title: t("contextmenu.task.copyFailed") });
    }
  };

  return (
    <RegionContextMenu
      items={
        <>
          <MenuItem icon={Clipboard} label={t("contextmenu.about.copyVersion")} onSelect={() => void copyVersion()} />
          <MenuItem
            icon={RotateCcw}
            label={t("about.checkForUpdates")}
            disabled={!updater.isTauri || updater.checking || updater.installing}
            onSelect={() => void updater.checkForUpdate()}
          />
          <MenuSeparator />
          <MenuItem
            icon={ExternalLink}
            label={t("contextmenu.about.openReleases")}
            onSelect={() => void window.open(`${REPO_URL}/releases`, "_blank", "noopener,noreferrer")}
          />
        </>
      }
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto bg-surface-root">
        <div className="mx-auto flex w-full max-w-[640px] flex-col gap-6 px-6 py-10 md:px-8 md:py-14">
          {/* ── Header: app name + version ── */}
          <header className="flex flex-col items-center gap-3 text-center">
            <img
              src="/logo-64.png"
              alt=""
              width={56}
              height={56}
              className="select-none rounded-2xl"
              draggable={false}
            />
            <h1 className="text-2xl font-semibold tracking-tight text-text-primary">{t("app.name")}</h1>
            <span className="inline-flex items-center rounded-full border border-border-subtle bg-surface-raised px-3 py-0.5 text-xs font-medium text-text-secondary">
              {t("about.version")} {version}
            </span>
            <p className="max-w-[480px] text-sm leading-relaxed text-text-secondary">{t("about.description")}</p>
          </header>

          {/* ── Divider ── */}
          <div className="h-px bg-border-subtle/60" />

          {/* ── Updates ── */}
          <section className="flex flex-col gap-3">
            <div className="flex flex-col gap-0.5">
              <h2 className="text-[11px] font-medium tracking-wide text-text-muted uppercase">{t("about.updates")}</h2>
              <p className="text-xs text-text-muted">{t("about.updatesDescription")}</p>
            </div>
            <div className="flex flex-col gap-3 rounded-lg border border-border-subtle/60 bg-surface-raised/40 px-4 py-3">
              <UpdateStatusBadge updater={updater} />
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8"
                  disabled={!updater.isTauri || updater.checking || updater.installing}
                  onClick={() => void updater.checkForUpdate()}
                >
                  {updater.checking ? (
                    <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <RotateCcw className="h-3.5 w-3.5" />
                  )}
                  {updater.checking ? t("about.checkingForUpdates") : t("about.checkForUpdates")}
                </Button>
                {updater.status === "available" ? (
                  <Button
                    type="button"
                    size="sm"
                    className="h-8"
                    disabled={updater.installing}
                    onClick={() => void updater.installUpdate()}
                  >
                    {updater.installing ? (
                      <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Sparkles className="h-3.5 w-3.5" />
                    )}
                    {t("about.installAndRestart")}
                  </Button>
                ) : null}
                {updater.status === "error" ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-8"
                    disabled={updater.checking}
                    onClick={() => void updater.checkForUpdate()}
                  >
                    <RotateCcw className="h-3.5 w-3.5" />
                    {t("about.retry")}
                  </Button>
                ) : null}
                {updater.status === "available" && !updater.installing ? (
                  <Button type="button" variant="ghost" size="sm" className="h-8" onClick={updater.dismissUpdate}>
                    <X className="h-3.5 w-3.5" />
                    {t("about.dismiss")}
                  </Button>
                ) : null}
              </div>
              {updater.status === "downloading" && updater.progress ? <UpdateProgressBar updater={updater} /> : null}
              {updater.status === "available" && updater.releaseNotes ? (
                <div className="flex flex-col gap-1">
                  <p className="text-[11px] font-medium tracking-wide text-text-muted uppercase">
                    {t("about.releaseNotes")}
                  </p>
                  <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-md border border-border-subtle bg-surface-root px-3 py-2 text-xs leading-5 text-text-secondary">
                    {updater.releaseNotes}
                  </pre>
                </div>
              ) : null}
              {updater.status === "available" && updater.updateDate ? (
                <p className="text-xs text-text-muted">
                  {t("about.released")}: {formatReleaseDate(updater.updateDate)}
                </p>
              ) : null}
            </div>
          </section>

          {/* ── Divider ── */}
          <div className="h-px bg-border-subtle/60" />

          {/* ── Author + License ── */}
          <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-1 rounded-lg border border-border-subtle/60 bg-surface-raised/40 px-4 py-3">
              <dt className="text-[11px] font-medium tracking-wide text-text-muted uppercase">{t("about.author")}</dt>
              <dd className="text-sm font-medium text-text-primary">{t("about.authorValue")}</dd>
            </div>
            <div className="flex flex-col gap-1 rounded-lg border border-border-subtle/60 bg-surface-raised/40 px-4 py-3">
              <dt className="text-[11px] font-medium tracking-wide text-text-muted uppercase">{t("about.license")}</dt>
              <dd className="text-sm font-medium text-text-primary">{t("about.licenseValue")}</dd>
            </div>
          </dl>

          {/* ── Divider ── */}
          <div className="h-px bg-border-subtle/60" />

          {/* ── Help ── */}
          <section className="flex flex-col gap-2">
            <h2 className="text-[11px] font-medium tracking-wide text-text-muted uppercase">{t("about.help")}</h2>
            <button
              type="button"
              onClick={onOpenOnboarding}
              className={cn(
                "group flex items-center gap-3 rounded-lg border border-transparent px-3 py-2.5 text-left",
                "text-sm text-text-secondary transition-colors duration-[var(--motion-ui)]",
                "hover:border-border-subtle hover:bg-surface-raised hover:text-text-primary",
              )}
            >
              <HelpCircle
                className="h-4 w-4 shrink-0 text-text-muted transition-colors group-hover:text-accent-primary"
                aria-hidden
              />
              <span className="flex-1">{t("about.viewOnboarding")}</span>
            </button>
          </section>

          {/* ── Divider ── */}
          <div className="h-px bg-border-subtle/60" />

          {/* ── External links ── */}
          <section className="flex flex-col gap-2">
            <h2 className="text-[11px] font-medium tracking-wide text-text-muted uppercase">{t("about.links")}</h2>
            <ul className="flex flex-col gap-1">
              {LINKS.map((link) => {
                const Icon = link.icon;
                return (
                  <li key={link.key}>
                    <a
                      href={link.href}
                      target="_blank"
                      rel="noopener noreferrer"
                      className={cn(
                        "group flex items-center gap-3 rounded-lg border border-transparent px-3 py-2.5",
                        "text-sm text-text-secondary transition-colors duration-[var(--motion-ui)]",
                        "hover:border-border-subtle hover:bg-surface-raised hover:text-text-primary",
                      )}
                    >
                      <Icon
                        className="h-4 w-4 shrink-0 text-text-muted transition-colors group-hover:text-accent-primary"
                        aria-hidden
                      />
                      <span className="flex-1">{t(`about.${link.key}`)}</span>
                      <ExternalLink
                        className="h-3.5 w-3.5 shrink-0 text-text-muted opacity-0 transition-opacity group-hover:opacity-100"
                        aria-hidden
                      />
                    </a>
                  </li>
                );
              })}
            </ul>
          </section>

          {/* ── Copyright ── */}
          <footer className="pt-2 text-center text-xs text-text-muted">{t("about.copyright")}</footer>
        </div>
      </div>
    </RegionContextMenu>
  );
}

type UpdaterSnapshot = ReturnType<typeof useAppUpdater>;

function UpdateStatusBadge({ updater }: { updater: UpdaterSnapshot }) {
  const { t } = useTranslation();

  if (updater.status === "checking") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("about.checkingForUpdates")}
      </span>
    );
  }
  if (updater.status === "up-to-date") {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <span className="inline-flex items-center gap-2 text-sm text-status-success">
          <Check className="h-4 w-4" />
          {t("about.upToDate")}
        </span>
        <span className="text-xs text-text-muted">{t("about.upToDateDescription")}</span>
      </span>
    );
  }
  if (updater.status === "available") {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <span className="inline-flex items-center gap-2 text-sm font-medium text-accent-primary">
          <Sparkles className="h-4 w-4" />
          {t("about.updateAvailable", { version: updater.updateVersion ?? "" })}
        </span>
        <span className="text-xs text-text-muted">{t("about.updateAvailableDescription")}</span>
      </span>
    );
  }
  if (updater.status === "downloading") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("about.downloading")}
      </span>
    );
  }
  if (updater.status === "installing") {
    return (
      <span className="inline-flex items-center gap-2 text-sm text-text-secondary">
        <LoaderCircle className="h-4 w-4 animate-spin text-accent-primary" />
        {t("about.installing")}
      </span>
    );
  }
  if (updater.status === "error") {
    return (
      <span className="inline-flex flex-col gap-0.5">
        <span className="inline-flex items-center gap-2 text-sm text-status-danger">
          <X className="h-4 w-4" />
          {t("about.updateFailed")}
        </span>
        {updater.error ? (
          <span className="truncate text-xs text-text-muted" title={updater.error}>
            {updater.error}
          </span>
        ) : null}
      </span>
    );
  }
  return <span className="text-sm text-text-muted">{t("about.checkForUpdates")}</span>;
}

function UpdateProgressBar({ updater }: { updater: UpdaterSnapshot }) {
  const { t } = useTranslation();
  const progress = updater.progress;
  if (!progress) return null;

  const downloaded = progress.downloadedBytes;
  const total = progress.totalBytes;
  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;

  const label =
    total && total > 0
      ? t("settings.updateProgress", {
          downloaded: formatBytes(downloaded),
          total: formatBytes(total),
        })
      : t("settings.updateProgressUnknown", { downloaded: formatBytes(downloaded) });

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-xs text-text-muted">
        <span>{label}</span>
        {percent !== null ? <span className="font-mono tabular-nums">{percent}%</span> : null}
      </div>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full bg-surface-root"
        role="progressbar"
        aria-valuenow={percent ?? 0}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className="h-full rounded-full bg-accent-primary transition-[width] duration-ui"
          style={{ width: `${percent ?? 100}%` }}
        />
      </div>
    </div>
  );
}

function formatReleaseDate(isoDate: string): string {
  try {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return isoDate;
    return date.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return isoDate;
  }
}
