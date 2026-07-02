import * as ContextMenu from "@radix-ui/react-context-menu";
import type { ComponentType } from "react";

import { cn } from "@/lib/utils";

/**
 * Shared context-menu primitives used by every region menu in the app.
 *
 * Visual spec is aligned with Popover / Select / Dialog:
 *   ring-1 ring-border-subtle + shadow-md + p-1.5 + fade-in animation.
 */
export function MenuItem({
  icon: Icon,
  label,
  shortcut,
  destructive,
  disabled,
  onSelect,
}: {
  icon?: ComponentType<{ className?: string }>;
  label: string;
  /** Optional right-aligned shortcut hint, e.g. "Ctrl+C" / "Delete". */
  shortcut?: string;
  destructive?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}) {
  return (
    <ContextMenu.Item
      disabled={disabled}
      className={cn(
        "flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none",
        "transition-colors duration-[var(--motion-ui)] ease-out",
        destructive
          ? "text-status-danger data-[highlighted]:bg-status-danger/10 data-[highlighted]:text-status-danger"
          : "text-text-secondary data-[highlighted]:bg-accent-primary/10 data-[highlighted]:text-text-primary",
        "data-[disabled]:pointer-events-none data-[disabled]:opacity-40",
      )}
      onSelect={onSelect}
    >
      {Icon && <Icon className={cn("h-4 w-4 shrink-0", destructive && "text-status-danger")} />}
      <span className="truncate">{label}</span>
      {shortcut && <span className="ml-auto pl-4 text-xs tracking-wide text-text-muted">{shortcut}</span>}
    </ContextMenu.Item>
  );
}

export function MenuSeparator({ className }: { className?: string }) {
  return <ContextMenu.Separator className={cn("my-1 h-px bg-border-divider", className)} />;
}

/** Shared content shell so every region menu looks the same. */
export function MenuContent({
  children,
  className,
  alignOffset,
}: {
  children: React.ReactNode;
  className?: string;
  alignOffset?: number;
}) {
  return (
    <ContextMenu.Content
      alignOffset={alignOffset ?? 4}
      className={cn(
        "min-w-48 overflow-hidden rounded-lg bg-surface-overlay p-1.5 shadow-md ring-1 ring-border-subtle",
        "data-[state=open]:animate-[fade-in_120ms_ease-out]",
        className,
      )}
    >
      {children}
    </ContextMenu.Content>
  );
}

/** Non-interactive section label inside a menu. */
export function MenuLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="select-none px-2 py-1 text-[11px] font-medium tracking-wide text-text-muted uppercase">
      {children}
    </div>
  );
}

/**
 * Convenience wrapper for "region" context menus (sidebar, status bar, title
 * bar, etc.) that just need a single trigger + content shell. Children are the
 * menu items; the trigger wraps the region's existing element.
 */
export function RegionContextMenu({
  children,
  items,
  className,
  alignOffset,
}: {
  /** Element to wrap as the context-menu trigger. */
  children: React.ReactNode;
  /** Menu items (MenuItem / MenuSeparator / MenuLabel). */
  items: React.ReactNode;
  className?: string;
  alignOffset?: number;
}) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <MenuContent className={className} alignOffset={alignOffset}>
          {items}
        </MenuContent>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
