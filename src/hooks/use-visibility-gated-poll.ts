import { useEffect, useRef } from "react";

/**
 * Runs `callback` on a fixed interval while the document is visible and
 * fully suspends it while hidden (PERF-14: a background window must not keep
 * issuing scheduler/IPC round-trips nobody can see).
 *
 * An in-flight guard prevents overlapping callbacks when one tick outlasts
 * the interval, and a visibilitychange listener refreshes immediately on
 * re-show instead of waiting up to one full interval.
 *
 * The callback is read through a ref so callers can pass an inline closure
 * without re-arming the timer on every render.
 *
 * Options:
 * - `enabled`: when false the effect does nothing at all — no initial load,
 *   no interval (used when the pane that needs the data is not visible).
 * - `poll`: when false the callback still runs once per `reloadKey` change
 *   but no interval is armed (used for tasks that are no longer live).
 * - `reloadKey`: change it to re-run the effect, e.g. when the task id or
 *   protocol changes.
 *
 * `isStale` lets the callback skip state updates after the effect has been
 * torn down, which is what the hand-rolled `cancelled` flags used to do.
 */
export function useVisibilityGatedPoll(
  callback: (isStale: () => boolean) => void | Promise<unknown>,
  intervalMs: number,
  options: { enabled?: boolean; poll?: boolean; reloadKey?: unknown } = {},
) {
  const { enabled = true, poll = true, reloadKey } = options;
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  // biome-ignore lint/correctness/useExhaustiveDependencies: `reloadKey` is the intentional rebuild trigger (task id/protocol change) and the callback is read through a ref, so neither belongs to the data flow the rule infers.
  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;
    let running = false;
    let timer: number | null = null;
    const isStale = () => cancelled;

    const tick = async () => {
      if (running || cancelled) return;
      running = true;
      try {
        await callbackRef.current(isStale);
      } finally {
        running = false;
      }
    };

    const stop = () => {
      if (timer !== null) {
        window.clearInterval(timer);
        timer = null;
      }
    };
    const start = () => {
      // Idempotent: repeated visibilitychange events while already visible
      // must not stack a second interval on top of the first.
      if (timer !== null) return;
      void tick();
      // Without `poll` this stays a one-shot load, matching detail panes that
      // only refresh while the task is downloading or retrying.
      if (!poll) return;
      timer = window.setInterval(() => void tick(), intervalMs);
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        stop();
      } else {
        start();
      }
    };

    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      cancelled = true;
      stop();
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [intervalMs, enabled, poll, reloadKey]);
}
