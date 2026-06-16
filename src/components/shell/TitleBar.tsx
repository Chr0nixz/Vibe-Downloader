import { Minus, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { usesCustomTitleBar, type Platform } from "@/lib/platform";
import {
  closeWindow,
  minimizeWindow,
  startWindowDrag,
  toggleMaximizeWindow,
} from "@/lib/window-controls";
import { useSettingsStore } from "@/stores/settings-store";
import { cn } from "@/lib/utils";

interface TitleBarProps {
  platform: Platform;
}

export function TitleBar({ platform }: TitleBarProps) {
  const { t } = useTranslation();
  const titlebarGradient = useSettingsStore(
    (s) => s.settings?.titlebarGradientEnabled ?? true,
  );

  if (platform === "linux") {
    return null;
  }

  const showWindowsControls = usesCustomTitleBar(platform);

  return (
    <header
      className={cn(
        "titlebar relative flex h-[var(--titlebar-height)] shrink-0 items-center bg-surface-base/90",
        titlebarGradient && "titlebar-gradient",
        platform === "macos" && "pl-[var(--traffic-lights-inset)]",
      )}
      data-tauri-drag-region
      onMouseDown={(event) => {
        if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
        void startWindowDrag();
      }}
    >
      <div
        className="flex min-w-0 flex-1 items-center gap-2.5 px-3"
        data-tauri-drag-region
      >
        <img
          src="/logo-48.png"
          alt=""
          width={20}
          height={20}
          className="shrink-0 select-none titlebar-logo"
          draggable={false}
        />
        <span className="truncate text-[13px] font-semibold tracking-wide text-text-primary">
          {t("app.name")}
        </span>
      </div>

      {showWindowsControls ? (
        <div className="flex items-center" data-no-drag>
          <WindowControl
            label={t("titleBar.minimize")}
            onClick={() => void minimizeWindow()}
          >
            <Minus className="h-3.5 w-3.5" />
          </WindowControl>
          <WindowControl
            label={t("titleBar.maximize")}
            onClick={() => void toggleMaximizeWindow()}
          >
            <Square className="h-3 w-3" />
          </WindowControl>
          <WindowControl
            label={t("titleBar.close")}
            onClick={() => void closeWindow()}
            className="hover:bg-status-danger hover:text-text-on-danger"
          >
            <X className="h-3.5 w-3.5" />
          </WindowControl>
        </div>
      ) : null}
    </header>
  );
}

function WindowControl({
  label,
  onClick,
  children,
  className,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className={cn(
            "h-[var(--titlebar-height)] w-11 rounded-none transition-colors duration-150 hover:bg-surface-raised",
            className,
          )}
          aria-label={label}
          onClick={onClick}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
