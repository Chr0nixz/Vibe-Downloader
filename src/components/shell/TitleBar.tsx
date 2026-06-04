import { Minus, Square, X } from "lucide-react";

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
import { cn } from "@/lib/utils";

interface TitleBarProps {
  platform: Platform;
}

export function TitleBar({ platform }: TitleBarProps) {
  if (platform === "linux") {
    return null;
  }

  const showWindowsControls = usesCustomTitleBar(platform);

  return (
    <header
      className={cn(
        "flex h-[var(--titlebar-height)] shrink-0 items-center border-b border-border-subtle bg-surface-base/90",
        platform === "macos" && "pl-[var(--traffic-lights-inset)]",
      )}
      data-tauri-drag-region
      onMouseDown={(event) => {
        if ((event.target as HTMLElement).closest("[data-no-drag]")) return;
        void startWindowDrag();
      }}
    >
      <div className="flex min-w-0 flex-1 items-center gap-2 px-3" data-tauri-drag-region>
        <span className="truncate text-sm font-medium text-text-primary">
          Vibe Downloader
        </span>
      </div>

      {showWindowsControls ? (
        <div className="flex items-center" data-no-drag>
          <WindowControl
            label="Minimize"
            onClick={() => void minimizeWindow()}
          >
            <Minus className="h-3.5 w-3.5" />
          </WindowControl>
          <WindowControl
            label="Maximize"
            onClick={() => void toggleMaximizeWindow()}
          >
            <Square className="h-3 w-3" />
          </WindowControl>
          <WindowControl
            label="Close"
            onClick={() => void closeWindow()}
            className="hover:bg-status-danger hover:text-white"
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
            "h-[var(--titlebar-height)] w-11 rounded-none hover:bg-surface-raised",
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
