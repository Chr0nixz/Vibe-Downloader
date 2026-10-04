import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  isOwnClipboardWrite,
  registerOwnClipboardWriteForTest,
  resetOwnClipboardWrites,
  writeClipboardText,
} from "./clipboard-write";

describe("clipboard-write own-write suppression (UX-43)", () => {
  beforeEach(() => {
    resetOwnClipboardWrites();
    vi.stubGlobal("navigator", {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("flags a detection whose URL came from an app-side write", () => {
    registerOwnClipboardWriteForTest("https://example.com/file.zip", Date.now());
    expect(isOwnClipboardWrite(["https://example.com/file.zip"])).toBe(true);
  });

  it("does not flag foreign URLs", () => {
    registerOwnClipboardWriteForTest("https://example.com/file.zip", Date.now());
    expect(isOwnClipboardWrite(["https://other.example.org/page"])).toBe(false);
  });

  it("treats mixed app-written and foreign URLs as foreign", () => {
    registerOwnClipboardWriteForTest("https://example.com/file.zip", Date.now());
    expect(isOwnClipboardWrite(["https://example.com/file.zip", "https://stranger.example/x"])).toBe(false);
  });

  it("matches URLs inside multi-line app writes (copy failed URLs)", () => {
    registerOwnClipboardWriteForTest("https://a.example/1.zip\nhttps://b.example/2.zip", Date.now());
    expect(isOwnClipboardWrite(["https://a.example/1.zip", "https://b.example/2.zip"])).toBe(true);
  });

  it("expires the suppression after the TTL", () => {
    const writeTime = Date.now();
    registerOwnClipboardWriteForTest("https://example.com/file.zip", writeTime);
    expect(isOwnClipboardWrite(["https://example.com/file.zip"], writeTime + 1_000)).toBe(true);
    expect(isOwnClipboardWrite(["https://example.com/file.zip"], writeTime + 30_000)).toBe(false);
  });

  it("writeClipboardText registers the text before writing", async () => {
    await writeClipboardText("https://example.com/task-url");
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("https://example.com/task-url");
    expect(isOwnClipboardWrite(["https://example.com/task-url"])).toBe(true);
  });

  it("empty detection lists are never own writes", () => {
    expect(isOwnClipboardWrite([])).toBe(false);
  });

  it("matches a backend-normalized URL that gained a trailing slash", () => {
    // "Copy download URL" writes the bare origin; the backend's Url::parse
    // round-trip adds a trailing slash before emitting the detection.
    registerOwnClipboardWriteForTest("https://example.com", Date.now());
    expect(isOwnClipboardWrite(["https://example.com/"])).toBe(true);
  });
});
