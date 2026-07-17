import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import * as React from "react";

import { cn } from "@/lib/utils";

const TooltipProvider = TooltipPrimitive.Provider;
const Tooltip = TooltipPrimitive.Root;
const TooltipTrigger = TooltipPrimitive.Trigger;

const TooltipContent = React.forwardRef<
  React.ComponentRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 4, ...props }, ref) => (
  <TooltipPrimitive.Content
    ref={ref}
    sideOffset={sideOffset}
    className={cn(
      // P1a: use ring-1 instead of border to avoid the codex "ghost-card" pattern
      // (1px border + box-shadow blur >= 16px on the same element). The global
      // rule in globals.css applies --shadow-popover (24px blur layer) to radix
      // tooltip content; pairing that with a literal border trips the ban.
      // Sibling popover.tsx and dialog.tsx already use ring-1 for this reason.
      "z-50 rounded-md bg-surface-overlay px-2 py-1 text-xs text-text-primary shadow-md ring-1 ring-border-subtle",
      className,
    )}
    {...props}
  />
));
TooltipContent.displayName = TooltipPrimitive.Content.displayName;

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
