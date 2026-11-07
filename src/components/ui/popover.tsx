import * as PopoverPrimitive from "@radix-ui/react-popover";
import { motion, useReducedMotion } from "motion/react";
import * as React from "react";

import { cn } from "@/lib/utils";

const Popover = PopoverPrimitive.Root;
const PopoverTrigger = PopoverPrimitive.Trigger;
const PopoverClose = PopoverPrimitive.Close;

const PopoverContent = React.forwardRef<
  React.ComponentRef<typeof PopoverPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className, align = "start", sideOffset = 4, ...props }, ref) => {
  const reduceMotion = useReducedMotion();

  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content ref={ref} align={align} sideOffset={sideOffset} asChild {...props}>
        <motion.div
          className={cn(
            // 12px outer radius = 6px item radius + 6px padding, so corners nest.
            "z-50 min-w-[8rem] rounded-xl bg-surface-popover p-1.5 shadow-md ring-1 ring-border-subtle",
            className,
          )}
          // Grow out of the trigger rather than the popover's own centre.
          style={{ transformOrigin: "var(--radix-popover-content-transform-origin)" }}
          initial={reduceMotion ? false : { opacity: 0, y: -4, scale: 0.97 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={reduceMotion ? { opacity: 0 } : { opacity: 0, y: -2, scale: 0.97 }}
          transition={{ duration: 0.16, ease: [0.16, 1, 0.3, 1] }}
        >
          {props.children}
        </motion.div>
      </PopoverPrimitive.Content>
    </PopoverPrimitive.Portal>
  );
});
PopoverContent.displayName = PopoverPrimitive.Content.displayName;

export { Popover, PopoverClose, PopoverContent, PopoverTrigger };
