import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-[color,background-color,border-color,box-shadow,transform] duration-[var(--motion-ui)] ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:ring-offset-1 focus-visible:ring-offset-surface-root active:scale-[0.97] active:duration-75 disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      variant: {
        // Default = solid accent fill + layered brand-tinted shadow (no border).
        // Replaces the prior `border + 0_1px_2px shadow + hover:brightness-110`
        // ghost-card anti-pattern with a single committed surface.
        default: "bg-accent-primary text-text-on-accent shadow-[var(--shadow-raised)] hover:bg-accent-primary/90",
        ghost: "hover:bg-surface-raised text-text-secondary hover:text-text-primary",
        outline: "border border-border-subtle bg-transparent hover:bg-surface-raised hover:border-border-hover",
        danger: "bg-status-danger/15 text-status-danger hover:bg-status-danger/25 font-semibold",
      },
      size: {
        default: "h-8 px-3.5",
        sm: "h-8 px-2.5 text-xs",
        icon: "h-8 w-8",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />;
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
