import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useVisibilityGatedPoll } from "./use-visibility-gated-poll";

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "hidden", {
    configurable: true,
    get: () => hidden,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("useVisibilityGatedPoll (PERF-14)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setHidden(false);
  });

  afterEach(() => {
    setHidden(false);
    vi.useRealTimers();
  });

  it("ticks immediately and on the interval while visible", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useVisibilityGatedPoll(callback, 10_000));

    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(callback).toHaveBeenCalledTimes(4);
  });

  it("stops polling while the document is hidden and resumes on re-show", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useVisibilityGatedPoll(callback, 10_000));

    act(() => setHidden(true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    // The immediate tick plus one interval tick before hiding; nothing after.
    expect(callback).toHaveBeenCalledTimes(1);

    await act(async () => {
      setHidden(false);
    });
    // Re-show fires one refresh right away, then the interval resumes.
    expect(callback).toHaveBeenCalledTimes(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(callback).toHaveBeenCalledTimes(4);
  });

  it("does not stack intervals when visibilitychange repeats while visible", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useVisibilityGatedPoll(callback, 10_000));

    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    // One immediate tick + exactly one interval tick despite duplicate events.
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("skips a tick while the previous callback is still in flight", async () => {
    let release: (() => void) | undefined;
    const callback = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    renderHook(() => useVisibilityGatedPoll(callback, 10_000));
    expect(callback).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(callback).toHaveBeenCalledTimes(1);

    await act(async () => {
      release?.();
    });
    // The in-flight guard drops overdue ticks; the next interval fires fresh.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(callback).toHaveBeenCalledTimes(2);
  });

  it("does nothing at all while disabled", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useVisibilityGatedPoll(callback, 10_000, { enabled: false }));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(callback).not.toHaveBeenCalled();
  });

  it("loads once and arms no interval when poll is false", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    renderHook(() => useVisibilityGatedPoll(callback, 10_000, { poll: false }));

    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    // Non-live tasks still need their one-shot load, never a repeating timer.
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it("reloads when reloadKey changes", async () => {
    const callback = vi.fn().mockResolvedValue(undefined);
    const { rerender } = renderHook(
      ({ key }: { key: string }) => useVisibilityGatedPoll(callback, 10_000, { poll: false, reloadKey: key }),
      { initialProps: { key: "task-a" } },
    );

    expect(callback).toHaveBeenCalledTimes(1);
    await act(async () => {
      rerender({ key: "task-b" });
    });
    expect(callback).toHaveBeenCalledTimes(2);
  });
});
