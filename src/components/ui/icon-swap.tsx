import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * Cross-fades between two glyphs that stand for one toggle (Pause ↔ Resume),
 * so pressing the button shows the state flip instead of a hard icon swap.
 * Nothing animates on first render: a row scrolling into view keeps still.
 */
export function IconSwap({
  swapKey,
  children,
  className,
}: {
  swapKey: string;
  children: ReactNode;
  className?: string;
}) {
  const reduceMotion = useReducedMotion();
  return (
    <span className={cn("relative inline-flex h-4 w-4 shrink-0", className)}>
      <AnimatePresence initial={false}>
        <motion.span
          key={swapKey}
          className="absolute inset-0 flex items-center justify-center"
          initial={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.5, rotate: -60 }}
          animate={{ opacity: 1, scale: 1, rotate: 0 }}
          exit={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.5, rotate: 60 }}
          transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
        >
          {children}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}
