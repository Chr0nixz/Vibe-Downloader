import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ClipboardLinkDetectedPayload } from "@/generated/bindings";
import { registerOwnClipboardWriteForTest, resetOwnClipboardWrites } from "@/lib/clipboard-write";
import { useToastStore } from "@/stores/toast-store";

const onClipboardLinkDetected = vi.hoisted(() => vi.fn());
const sendSystemNotification = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  onClipboardLinkDetected: (...args: unknown[]) => onClipboardLinkDetected(...args),
}));

vi.mock("@/lib/system-notification", () => ({
  sendSystemNotification: (...args: unknown[]) => sendSystemNotification(...args),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return { ...actual, useTranslation: () => ({ t: (key: string) => key }) };
});

import { useClipboardLinkPrompt } from "./use-clipboard-link-prompt";

function payload(id: string, urls: string[]): ClipboardLinkDetectedPayload {
  return { id, urls, primaryUrl: urls[0] ?? "", detectedAt: "2026-09-29T00:00:00.000Z" };
}

let visibility: DocumentVisibilityState = "visible";

function setVisibility(next: DocumentVisibilityState) {
  visibility = next;
  document.dispatchEvent(new Event("visibilitychange"));
}

async function mountPrompt(onAccept = vi.fn()) {
  renderHook(() => useClipboardLinkPrompt(onAccept));
  await act(async () => {});
  const emit = onClipboardLinkDetected.mock.calls[0][0] as (payload: ClipboardLinkDetectedPayload) => void;
  return { onAccept, emit: (next: ClipboardLinkDetectedPayload) => act(() => emit(next)) };
}

describe("clipboard link prompt (UX-43)", () => {
  beforeEach(() => {
    visibility = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    onClipboardLinkDetected.mockReset();
    onClipboardLinkDetected.mockResolvedValue(vi.fn());
    sendSystemNotification.mockReset();
    resetOwnClipboardWrites();
    useToastStore.setState({ toasts: [] });
  });

  afterEach(() => {
    // Restore the jsdom prototype getter shadowed above.
    Reflect.deleteProperty(document, "visibilityState");
  });

  it("prompts with a toast and opens nothing until the user accepts", async () => {
    const { onAccept, emit } = await mountPrompt();
    const detected = payload("evt-1", ["https://example.com/a.zip"]);

    await emit(detected);

    const toasts = useToastStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].tone).toBe("info");
    expect(toasts[0].title).toBe("toast.clipboardLinkDetected");
    expect(onAccept).not.toHaveBeenCalled();

    act(() => toasts[0].action?.onClick());
    expect(onAccept).toHaveBeenCalledWith(detected);
  });

  it("drops the app's own clipboard writes", async () => {
    const { onAccept, emit } = await mountPrompt();
    registerOwnClipboardWriteForTest("https://example.com/own.iso", Date.now());

    await emit(payload("evt-own", ["https://example.com/own.iso"]));

    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(onAccept).not.toHaveBeenCalled();
  });

  it("uses the batch copy for multi-link detections", async () => {
    const { emit } = await mountPrompt();

    await emit(payload("evt-batch", ["https://example.com/1.zip", "https://example.com/2.zip"]));

    const [toast] = useToastStore.getState().toasts;
    expect(toast.title).toBe("toast.clipboardLinksDetected");
    expect(toast.description).toBe("toast.clipboardLinksDetectedDescription");
  });

  it("holds the latest detection while hidden and shows it once visible", async () => {
    const { onAccept, emit } = await mountPrompt();
    act(() => setVisibility("hidden"));

    await emit(payload("evt-old", ["https://example.com/old.zip"]));
    await emit(payload("evt-new", ["https://example.com/new.zip"]));
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(sendSystemNotification).toHaveBeenCalledTimes(1);
    expect(sendSystemNotification).toHaveBeenCalledWith("toast.clipboardLinkDetected", "toast.useClipboardLink", {
      requestPermission: false,
    });
    expect(sendSystemNotification.mock.calls.flat().join(" ")).not.toContain("example.com");

    act(() => setVisibility("visible"));
    const toasts = useToastStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0].description).toBe("https://example.com/new.zip");
    expect(onAccept).not.toHaveBeenCalled();

    // The held prompt is shown once, not again on the next visibility flip.
    act(() => setVisibility("hidden"));
    act(() => setVisibility("visible"));
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });
});
