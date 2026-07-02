import * as React from "react";

import { cn } from "@/lib/utils";

/**
 * Checkbox primitive built on a native `<input type="checkbox">`.
 *
 * The native chrome is replaced with a brand-styled box + check glyph via
 * the `.ui-checkbox` pseudo-element rules in `globals.css`. `accent-color`
 * is also set so that user stylesheets / forced-colors modes that bypass
 * `appearance: none` still render with the brand accent.
 *
 * `indeterminate` is supported as a first-class prop (the native element
 * only exposes it as an IDL attribute, so React can't set it declaratively;
 * we sync it via a ref callback).
 *
 * Use this anywhere a checkbox affordance is needed. For "click anywhere on
 * a row toggles the box" patterns, wrap the Checkbox and the row content in
 * a `<label>` rather than synthesizing a button-div with `role="checkbox"`.
 */
export interface CheckboxProps extends Omit<React.ComponentProps<"input">, "type"> {
  type?: "checkbox";
  /** Tri-state indeterminate. Renders a dash; supersedes `checked` visually. */
  indeterminate?: boolean;
}

const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  ({ className, type = "checkbox", indeterminate, ...props }, ref) => {
    const internalRef = React.useRef<HTMLInputElement | null>(null);
    React.useImperativeHandle(ref, () => internalRef.current as HTMLInputElement, []);
    React.useEffect(() => {
      if (internalRef.current) internalRef.current.indeterminate = !!indeterminate;
    }, [indeterminate]);

    return (
      <input
        type={type}
        ref={internalRef}
        className={cn(
          "ui-checkbox h-4 w-4 shrink-0 cursor-pointer rounded border border-border-subtle bg-surface-base accent-accent-primary",
          "transition-[background-color,border-color] duration-ui ease-out",
          "checked:border-accent-primary checked:bg-accent-primary",
          "indeterminate:border-accent-primary indeterminate:bg-accent-primary",
          "hover:border-accent-primary",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary",
          "disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      />
    );
  },
);
Checkbox.displayName = "Checkbox";

export { Checkbox };
