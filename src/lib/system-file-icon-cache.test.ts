import { describe, expect, it } from "vitest";

import { SystemFileIconCache, systemFileIconCacheKey } from "./system-file-icon-cache";

describe("systemFileIconCacheKey", () => {
  it("shares one normalized key for file names with the same extension", () => {
    expect(systemFileIconCacheKey("C:\\Downloads\\Movie.MKV")).toBe(".mkv");
    expect(systemFileIconCacheKey("/tmp/archive/movie.mkv")).toBe(".mkv");
  });

  it("uses a stable key for names without a usable extension", () => {
    expect(systemFileIconCacheKey("README")).toBe("__no_extension__");
    expect(systemFileIconCacheKey(".gitignore")).toBe("__no_extension__");
    expect(systemFileIconCacheKey("trailing.")).toBe("__no_extension__");
  });
});

describe("SystemFileIconCache", () => {
  it("evicts the least recently used entry at capacity", () => {
    const cache = new SystemFileIconCache(2);
    cache.set(".zip", "zip");
    cache.set(".pdf", "pdf");
    expect(cache.get(".zip")).toBe("zip");

    cache.set(".mkv", "mkv");

    expect(cache.get(".pdf")).toBeUndefined();
    expect(cache.get(".zip")).toBe("zip");
    expect(cache.size).toBe(2);
  });

  it("retains a cached null without treating it as a miss", () => {
    const cache = new SystemFileIconCache(2);
    cache.set(".unknown", null);
    expect(cache.get(".unknown")).toBeNull();
  });
});
