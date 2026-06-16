import { describe, expect, it } from "vitest";

import { normalizeTask } from "./task";

describe("normalizeTask", () => {
  it("defensively fills optional task arrays for malformed browser preview data", () => {
    const normalized = normalizeTask({
      id: "legacy-task",
      totalSize: "2048",
      downloadedBytes: "512",
      speedBps: "128",
    } as never);

    expect(normalized.files).toEqual([]);
    expect(normalized.recoveryActions).toEqual([]);
    expect(normalized.checksums).toEqual([]);
    expect(normalized.totalSize).toBe(2048);
    expect(normalized.downloadedBytes).toBe(512);
    expect(normalized.speedBps).toBe(128);
  });

  it("normalizes malformed task file byte fields without dropping files", () => {
    const normalized = normalizeTask({
      totalSize: null,
      downloadedBytes: null,
      speedBps: null,
      files: [
        {
          path: "movie.mkv",
          totalSize: "4096",
          downloadedBytes: undefined,
        },
      ],
    } as never);

    expect(normalized.files).toHaveLength(1);
    expect(normalized.files[0]?.totalSize).toBe(4096);
    expect(normalized.files[0]?.downloadedBytes).toBe(0);
  });
});
