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

  // Regression: a generic `zh-*` branch used to hand every Hant tag Simplified.
  it("stored 的 zh-Hant* 必须解析为 zh-TW，而不是简体", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    for (const stored of ["zh-Hant", "zh-Hant-TW", "zh-Hant-HK"]) {
      vi.stubGlobal("localStorage", {
        getItem: (key: string) => (key === "vibe-locale" ? stored : null),
        setItem: () => undefined,
        removeItem: () => undefined,
      });
      expect(detectInitialLocale()).toBe("zh-TW");
    }
  });

  it("navigator=zh-Hant 不回落到简体，而是按 beta 规则回落 en", () => {
    vi.stubGlobal("navigator", { language: "zh-Hant" });
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("en");
  });

  it("大小写不敏感：stored=ZH-HANT-TW → zh-TW", () => {
    vi.stubGlobal("navigator", { language: "en-US" });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "vibe-locale" ? "ZH-HANT-TW" : null),
      setItem: () => undefined,
      removeItem: () => undefined,
    });
    expect(detectInitialLocale()).toBe("zh-TW");
  });
});
