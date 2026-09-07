/**
 * Tracks how many modal surfaces currently own focus.
 *
 * A toast that fires while a dialog is open renders behind the scrim: the user
 * cannot read it, and its auto-dismiss timer expires before the dialog closes,
 * so the confirmation is simply lost. The toast store consults this counter to
 * hold non-error toasts until the last modal unmounts.
 */

let depth = 0;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

/**
 * Register a modal surface. Returns a release function; calling it more than
 * once is a no-op, so it is safe to hand straight to a React effect cleanup.
 */
export function acquireModalFocus(): () => void {
  depth += 1;
  if (depth === 1) emit();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    depth -= 1;
    if (depth === 0) emit();
  };
}

export function isModalFocusActive(): boolean {
  return depth > 0;
}

export function subscribeModalFocus(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
