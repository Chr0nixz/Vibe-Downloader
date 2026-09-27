import { describe, expect, it } from "vitest";

import { chunkCounts, chunkMapCells, chunkTone, hasByteRangeSegments, interpolateChunkCells } from "@/lib/chunk-map";
import type { TaskSegment } from "@/types/task-segment";

function segment(overrides: Partial<TaskSegment> & Pick<TaskSegment, "id" | "rangeStart" | "rangeEnd">): TaskSegment {
  return {
    taskId: "t",
    fileId: null,
    unitKind: "range",
    downloadedUntil: overrides.rangeStart,
    speedBps: 0,
    status: "pending",
    retryCount: 0,
    lastError: null,
    ...overrides,
  };
}

describe("chunkMapCells", () => {
  it("places each range at its offset in the file, sorted, with its written share", () => {
    const cells = chunkMapCells(
      [
        segment({ id: "b", rangeStart: 500, rangeEnd: 999, downloadedUntil: 750, status: "downloading" }),
        segment({ id: "a", rangeStart: 0, rangeEnd: 499, downloadedUntil: 500, status: "completed" }),
      ],
      1000,
      "downloading",
    );
    expect(cells?.map((cell) => cell.id)).toEqual(["a", "b"]);
    expect(cells?.[0]).toMatchObject({ leftPct: 0, widthPct: 50, fill: 1, tone: "done" });
    expect(cells?.[1]).toMatchObject({ leftPct: 50, widthPct: 50, fill: 0.5, tone: "live" });
  });

  it("refuses to draw a map it cannot draw truthfully", () => {
    const two = [
      segment({ id: "a", rangeStart: 0, rangeEnd: 499 }),
      segment({ id: "b", rangeStart: 500, rangeEnd: 999 }),
    ];
    // One range is just the progress bar.
    expect(chunkMapCells(two.slice(0, 1), 1000, "downloading")).toBeNull();
    // Unknown size.
    expect(chunkMapCells(two, 0, "downloading")).toBeNull();
    // Per-file segments (Metalink) are not ranges of one file.
    expect(chunkMapCells([{ ...two[0], fileId: "f1" }, two[1]], 1000, "downloading")).toBeNull();
    // A range past the end of the file.
    expect(chunkMapCells([two[0], { ...two[1], rangeEnd: 1000 }], 1000, "downloading")).toBeNull();
  });

  it("clamps fills so stale or overshooting progress never paints outside a range", () => {
    const cells = chunkMapCells(
      [
        segment({ id: "a", rangeStart: 0, rangeEnd: 99, downloadedUntil: 400, status: "downloading" }),
        segment({ id: "b", rangeStart: 100, rangeEnd: 199, downloadedUntil: 50, status: "pending" }),
      ],
      200,
      "downloading",
    );
    expect(cells?.map((cell) => cell.fill)).toEqual([1, 0]);
  });
});

describe("chunkTone", () => {
  it("keeps the energy accent for connections writing right now", () => {
    expect(chunkTone("downloading", "downloading")).toBe("live");
    expect(chunkTone("downloading", "retrying")).toBe("live");
    expect(chunkTone("completed", "downloading")).toBe("done");
  });

  it("greys out ranges of a task that is not moving and flags failures", () => {
    expect(chunkTone("downloading", "paused")).toBe("idle");
    expect(chunkTone("completed", "queued")).toBe("idle");
    expect(chunkTone("failed", "paused")).toBe("failed");
    expect(chunkTone("completed", "completed")).toBe("complete");
  });
});

describe("hasByteRangeSegments", () => {
  it("covers HTTP-family and FTP, not playlist, piece or single-stream engines", () => {
    for (const protocol of ["http", "https", "webdav", "webdavs", "ftp", "ftps"]) {
      expect(hasByteRangeSegments(protocol), protocol).toBe(true);
    }
    for (const protocol of ["sftp", "hls", "dash", "bt", "magnet", "metalink"]) {
      expect(hasByteRangeSegments(protocol), protocol).toBe(false);
    }
  });
});

describe("chunkCounts", () => {
  it("counts by segment status", () => {
    expect(
      chunkCounts([
        segment({ id: "a", rangeStart: 0, rangeEnd: 1, status: "completed" }),
        segment({ id: "b", rangeStart: 2, rangeEnd: 3, status: "downloading" }),
        segment({ id: "c", rangeStart: 4, rangeEnd: 5, status: "failed" }),
      ]),
    ).toEqual({ total: 3, completed: 1, active: 1, failed: 1 });
  });
});

describe("interpolateChunkCells", () => {
  const cellsFor = (speeds: [number, number]) =>
    chunkMapCells(
      [
        segment({
          id: "a",
          rangeStart: 0,
          rangeEnd: 499,
          downloadedUntil: 100,
          status: "downloading",
          speedBps: speeds[0],
        }),
        segment({
          id: "b",
          rangeStart: 500,
          rangeEnd: 999,
          downloadedUntil: 600,
          status: "downloading",
          speedBps: speeds[1],
        }),
      ],
      1000,
      "downloading",
    ) ?? [];

  it("spreads bytes that arrived after the poll over the live ranges by connection speed", () => {
    // Poll saw 200 bytes written; the task total has since reached 500.
    const next = interpolateChunkCells(cellsFor([300, 100]), 500);
    expect(next[0].fill).toBeCloseTo((100 + 225) / 500);
    expect(next[1].fill).toBeCloseTo((100 + 75) / 500);
  });

  it("never overfills a range and never shrinks one when the total lags the poll", () => {
    const capped = interpolateChunkCells(cellsFor([1000, 0]), 2000);
    expect(capped[0].fill).toBe(1);
    const lagging = interpolateChunkCells(cellsFor([300, 100]), 50);
    expect(lagging.map((cell) => cell.fill)).toEqual([0.2, 0.2]);
  });
});
