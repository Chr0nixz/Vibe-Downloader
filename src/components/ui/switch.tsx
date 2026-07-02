import * as React from "react";

import { cn } from "@/lib/utils";

/**
 * Switch primitive — an on/off **state** toggle (not a selection).
 *
 * Built as a native `<button role="switch">` so it is keyboard-operable by
 * default (Space/Enter flip the state) and exposes `aria-checked`. Use this
 * for binary runtime states such as "seeding on/off". For multi-select
 * affordances, use {@link Checkbox} instead.
 *
 * States covered: default / hover / focus-visible / active / disabled, with a
 * `prefers-reduced-motion` override (see `.ui-switch` in `globals.css`).
 */
export interface SwitchProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "onChange" | "value" | "type"> {
  /** Whether the switch is on. */
  checked: boolean;
  /** Called with the next checked state. */
  onCheckedChange: (checked: boolean) => void;
}

const Switch = React.forwardRef<HTMLButtonElement, SwitchProps>(
  ({ className, checked, disabled, onCheckedChange, ...props }, ref) => {
    return (
      <button
        ref={ref}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onCheckedChange(!checked)}
        className={cn(
          "ui-switch relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border",
          "transition-[background-color,border-color] duration-[var(--motion-ui)] ease-out",
          "hover:border-accent-primary",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary focus-visible:ring-offset-1 focus-visible:ring-offset-surface-root",
          "active:translate-y-px",
          "disabled:cursor-not-allowed disabled:opacity-50",
          checked ? "border-transparent bg-accent-primary" : "border-border-subtle bg-surface-hover",
          className,
        )}
        {...props}
      >
        <span
          aria-hidden
          className={cn(
            "pointer-events-none ml-0.5 inline-block h-4 w-4 rounded-full",
            "transition-[transform,background-color] duration-[var(--motion-ui)] ease-out",
            checked ? "translate-x-4 bg-text-on-accent" : "translate-x-0 bg-text-secondary",
          )}
        />
      </button>
    );
  },
);
Switch.displayName = "Switch";

export { Switch };
