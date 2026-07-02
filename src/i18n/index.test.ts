import { afterEach, describe, expect, it, vi } from "vitest";

import { detectInitialLocale } from "./index";

describe("detectInitialLocale (UX-1)", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("navigator=ja 且无 stored → 回落 en", () => {
    vi.stubGlobal("navigator", { language: "ja" });
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("en");
  });

  it("navigator=zh-CN 且无 stored → zh-CN", () => {
    vi.stubGlobal("navigator", { language: "zh-CN" });
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("zh-CN");
  });

  it("navigator=en-US 且无 stored → en", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("en");
  });

  it("navigator=ko 且无 stored → en（beta 不自动检测）", () => {
    vi.stubGlobal("navigator", { language: "ko" });
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("en");
  });

  it("stored=ja → 尊重显式选择 ja", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "vibe-locale" ? "ja" : null),
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("ja");
  });

  it("stored=zh-TW → 尊重显式选择 zh-TW", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "vibe-locale" ? "zh-TW" : null),
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("zh-TW");
  });

  it("stored=zh → 规范化为 zh-CN", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "vibe-locale" ? "zh" : null),
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("zh-CN");
  });
});
