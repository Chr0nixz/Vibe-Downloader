import type { ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * A polite screen-reader announcement rendered straight into `<body>`.
 *
 * Radix modals hide the rest of the page with `aria-hidden`, but they keep
 * every `[aria-live]` element and its ancestors exposed so announcements are
 * not lost. A live region inside `#root` therefore kept `#root` itself out of
 * `aria-hidden`, and a screen reader's virtual cursor could wander out of an
 * open dialog. Living outside `#root` lets the whole app shell hide cleanly.
 */
export function LiveRegion({ children }: { children: ReactNode }) {
  if (typeof document === "undefined") return null;
  return createPortal(
    <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">
      {children}
    </div>,
    document.body,
  );
}
