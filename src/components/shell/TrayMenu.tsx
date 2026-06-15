import {
  AppWindow,
  FolderOpen,
  Plus,
  Power,
  Settings,
  X,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { TrayMenuAction } from "@/generated/bindings";
import { cn } from "@/lib/utils";
import { createLogger } from "@/lib/logger";
import { runTrayMenuAction } from "@/lib/tauri";

const log = createLogger("tray-menu");

interface TrayMenuItem {
  action: TrayMenuAction;
  labelKey: `trayMenu.${string}`;
  icon: LucideIcon;
  tone?: "primary" | "danger";
}

const items: TrayMenuItem[] = [
  {
    action: "newDownload",
    labelKey: "trayMenu.newDownload",
    icon: Plus,
    tone: "primary",
  },
  {
    action: "openApp",
    labelKey: "trayMenu.openApp",
    icon: AppWindow,
  },
  {
    action: "openDownloads",
    labelKey: "trayMenu.openDownloads",
    icon: FolderOpen,
  },
  {
    action: "settings",
    labelKey: "trayMenu.settings",
    icon: Settings,
  },
  {
    action: "quit",
    labelKey: "trayMenu.quit",
    icon: Power,
    tone: "danger",
  },
];

export function TrayMenu() {
  const { t } = useTranslation();
  const [pendingAction, setPendingAction] = useState<TrayMenuAction | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void hideWindow();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  async function runAction(action: TrayMenuAction) {
    if (pendingAction) return;
    setPendingAction(action);
    try {
      await runTrayMenuAction(action);
    } catch (err) {
      log.error("tray menu action failed", action, err);
    } finally {
      setPendingAction(null);
    }
  }

  return (
    <main className="flex h-full items-center justify-center p-1.5">
      <section
        className="w-full overflow-hidden rounded-lg border border-border-container bg-surface-overlay backdrop-blur-xl"
        aria-label={t("trayMenu.title")}
        role="dialog"
      >
        <div className="flex h-10 items-center gap-2.5 border-b border-border-divider px-2.5">
          <div
            className="grid h-7 w-7 place-items-center rounded-md bg-accent-primary/12 text-accent-primary ring-1 ring-accent-primary/25"
            aria-hidden
          >
            <span className="h-2 w-2 rounded-full bg-accent-energy" />
          </div>
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-[0.82rem] font-semibold leading-5 text-text-primary">
              {t("trayMenu.title")}
            </h1>
          </div>
          <button
            type="button"
            className="grid h-7 w-7 place-items-center rounded-md text-text-muted outline-none transition hover:bg-surface-raised hover:text-text-primary focus-visible:ring-2 focus-visible:ring-accent-primary"
            aria-label={t("trayMenu.close")}
            onClick={() => void hideWindow()}
          >
            <X className="h-4 w-4" aria-hidden />
          </button>
        </div>

        <nav className="p-1" role="group" aria-label={t("trayMenu.title")}>
          {items.map((item, index) => (
            <TrayMenuButton
              key={item.action}
              item={item}
              pending={pendingAction === item.action}
              separated={index === 4}
              onClick={() => void runAction(item.action)}
            />
          ))}
        </nav>
      </section>
    </main>
  );
}

function TrayMenuButton({
  item,
  pending,
  separated,
  onClick,
}: {
  item: TrayMenuItem;
  pending: boolean;
  separated: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  const Icon = item.icon;

  return (
    <button
      type="button"
      className={cn(
        "group flex h-9 w-full items-center gap-2.5 rounded-md px-2 text-left outline-none transition duration-ui ease-out hover:bg-surface-raised focus-visible:ring-2 focus-visible:ring-accent-primary",
        separated && "mt-1 border-t border-border-separator pt-1.5",
        item.tone === "danger" && "hover:bg-status-danger/10",
      )}
      disabled={pending}
      onClick={onClick}
    >
      <span
        className={cn(
          "grid h-7 w-7 shrink-0 place-items-center rounded-md bg-surface-raised text-text-secondary ring-1 ring-border-divider transition",
          item.tone === "primary" &&
            "bg-accent-primary text-text-on-accent ring-accent-primary",
          item.tone === "danger" &&
            "text-status-danger group-hover:bg-status-danger/14 group-hover:ring-status-danger/30",
        )}
        aria-hidden
      >
        <Icon className="h-3.5 w-3.5" />
      </span>
      <span className="min-w-0 flex-1">
        <span
          className={cn(
            "block truncate text-[0.8rem] font-medium leading-5 text-text-primary",
            item.tone === "danger" && "group-hover:text-status-danger",
          )}
        >
          {pending ? t("trayMenu.working") : t(item.labelKey)}
        </span>
      </span>
    </button>
  );
}

async function hideWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().hide();
}
