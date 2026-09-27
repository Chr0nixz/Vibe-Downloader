const OVERLAY_SELECTOR = '[role="dialog"], [role="alertdialog"], [role="menu"]';

/**
 * Whether a window-level keydown already belongs to a dialog, menu, or popover
 * layer, so the shell's list shortcuts must leave it alone.
 *
 * Keys pressed inside a layer are that layer's: Del in the shortcut panel used
 * to delete the selected task behind the scrim. Esc is also spent once Radix
 * has dismissed a layer with it (Radix calls preventDefault before closing),
 * so one Esc closes one layer instead of also closing the details panel.
 */
export function isOverlayKey(event: KeyboardEvent): boolean {
  const target = event.target;
  if (target instanceof Element && target.closest(OVERLAY_SELECTOR)) return true;
  return event.key === "Escape" && event.defaultPrevented;
}
