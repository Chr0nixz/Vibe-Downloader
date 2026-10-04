import type { Platform } from "@/lib/platform";

const OVERLAY_SELECTOR = '[role="dialog"], [role="alertdialog"], [role="menu"]';

/**
 * Whether the focused element owns text editing. Global paste should leave
 * these surfaces to the browser so a URL can still be pasted into a form.
 */
export function isTextInputTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest('textarea, select, [role="textbox"]')) return true;
  const editable = target.closest("[contenteditable]");
  if (
    (target instanceof HTMLElement && target.isContentEditable) ||
    (editable !== null &&
      ["", "true", "plaintext-only"].includes(editable.getAttribute("contenteditable")?.toLowerCase() ?? ""))
  ) {
    return true;
  }
  if (!(target instanceof HTMLInputElement)) return false;
  return !["button", "checkbox", "file", "image", "radio", "range", "reset", "submit"].includes(
    target.type.toLowerCase(),
  );
}

/** Global paste is available from the task surface, but never from a text
 * editor or a Radix overlay that owns the key event. */
export function isGlobalPasteShortcut(event: KeyboardEvent, platform: Platform): boolean {
  const modifier = platform === "macos" ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
  return (
    modifier &&
    !event.shiftKey &&
    !event.altKey &&
    !event.isComposing &&
    !event.repeat &&
    !event.defaultPrevented &&
    event.key.toLowerCase() === "v" &&
    !isTextInputTarget(event.target) &&
    !isOverlayKey(event)
  );
}

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
