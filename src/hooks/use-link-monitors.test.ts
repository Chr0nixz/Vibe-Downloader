import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const onClipboardLinkDetected = vi.hoisted(() => vi.fn());
const onFileDrop = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  onClipboardLinkDetected: (...args: unknown[]) => onClipboardLinkDetected(...args),
  onFileDrop: (...args: unknown[]) => onFileDrop(...args),
}));

import { useClipboardLinkMonitor } from "./use-clipboard-link-monitor";
import { useFileDropMonitor } from "./use-file-drop-monitor";

const unlisten = vi.fn();
const payload = { id: "evt-1", urls: ["https://example.com/a.zip"], primaryUrl: "https://example.com/a.zip" };

describe("clipboard link monitor (UX-23)", () => {
  beforeEach(() => {
    onClipboardLinkDetected.mockReset();
    unlisten.mockReset();
    onClipboardLinkDetected.mockResolvedValue(unlisten);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("registers once and dispatches events to the latest handler", async () => {
    const first = vi.fn();
    const second = vi.fn();

    const { rerender } = renderHook(({ handler }) => useClipboardLinkMonitor(handler), {
      initialProps: { handler: first },
    });
    await act(async () => {});
    expect(onClipboardLinkDetected).toHaveBeenCalledTimes(1);

    // Re-renders (dialog state, locale) must not tear the listener down.
    rerender({ handler: second });
    expect(onClipboardLinkDetected).toHaveBeenCalledTimes(1);

    await act(async () => {
      onClipboardLinkDetected.mock.calls[0][0](payload);
    });
    expect(second).toHaveBeenCalledWith(payload);
    expect(first).not.toHaveBeenCalled();
  });

  it("unlistens when registration resolves after unmount", async () => {
    let resolveRegister!: (value: () => void) => void;
    onClipboardLinkDetected.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveRegister = resolve;
        }),
    );

    const { unmount } = renderHook(() => useClipboardLinkMonitor(vi.fn()));
    unmount();
    await act(async () => {
      resolveRegister(unlisten);
    });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});

describe("file drop monitor (UX-23)", () => {
  beforeEach(() => {
    onFileDrop.mockReset();
    unlisten.mockReset();
    onFileDrop.mockResolvedValue(unlisten);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("registers once and forwards both event kinds to the latest handlers", async () => {
    const drop1 = vi.fn();
    const drop2 = vi.fn();
    const drag1 = vi.fn();
    const drag2 = vi.fn();

    const { rerender } = renderHook(({ handlers }) => useFileDropMonitor(handlers), {
      initialProps: { handlers: { onDrop: drop1, onDragStateChange: drag1 } },
    });
    await act(async () => {});
    expect(onFileDrop).toHaveBeenCalledTimes(1);

    rerender({ handlers: { onDrop: drop2, onDragStateChange: drag2 } });
    expect(onFileDrop).toHaveBeenCalledTimes(1);

    const [dropHandler, dragHandler] = onFileDrop.mock.calls[0];
    await act(async () => {
      dropHandler(["C:/downloads/a.zip"]);
    });
    expect(drop2).toHaveBeenCalledWith(["C:/downloads/a.zip"]);
    expect(drop1).not.toHaveBeenCalled();

    await act(async () => {
      dragHandler({ active: true });
    });
    expect(drag2).toHaveBeenCalledWith({ active: true });
    expect(drag1).not.toHaveBeenCalled();
  });
});
