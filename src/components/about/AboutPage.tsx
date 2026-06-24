import { BookOpen, Bug, Code, ExternalLink, Info } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getAppVersion } from "@/lib/tauri";
import { cn } from "@/lib/utils";

const REPO_URL = "https://github.com/Chr0nixz/Vibe-Downloader";
const LINKS = [
  { key: "linksGithub", href: REPO_URL, icon: Code },
  { key: "linksDocs", href: `${REPO_URL}#readme`, icon: BookOpen },
  { key: "linksIssues", href: `${REPO_URL}/issues`, icon: Bug },
] as const;

export function AboutPage() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string>("0.1.1");

  useEffect(() => {
    let cancelled = false;
    getAppVersion()
      .then((v) => {
        if (!cancelled) setVersion(v);
      })
      .catch(() => {
        /* keep fallback version */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto bg-surface-root">
      <div className="mx-auto flex w-full max-w-[640px] flex-col gap-6 px-6 py-10 md:px-8 md:py-14">
        {/* ── Header: app name + version ── */}
        <header className="flex flex-col items-center gap-3 text-center">
          <span className="flex h-14 w-14 items-center justify-center rounded-2xl bg-accent-primary/10 text-accent-primary">
            <Info className="h-7 w-7" aria-hidden />
          </span>
          <h1 className="text-2xl font-semibold tracking-tight text-text-primary">{t("app.name")}</h1>
          <span className="inline-flex items-center rounded-full border border-border-subtle bg-surface-raised px-3 py-0.5 text-xs font-medium text-text-secondary">
            {t("about.version")} {version}
          </span>
          <p className="max-w-[480px] text-sm leading-relaxed text-text-secondary">{t("about.description")}</p>
        </header>

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
  );
}
