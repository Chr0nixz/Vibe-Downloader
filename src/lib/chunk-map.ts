//! Chunk map layout (DESIGN.md "Data Visualization": chunk heatmaps for range
//! progress). Pure geometry so the overview strip and its tests agree on how a
//! segment list becomes cells; the component only paints them.

import type { TaskStatus } from "@/generated/bindings";
import { isFtpSftpProtocol, isHttpLikeProtocol } from "@/lib/task-diagnostics";
import type { TaskSegment } from "@/types/task-segment";

/** How a cell is painted. `live` is the only high-energy tone: a connection
 * writing right now. Everything settled or waiting stays quiet. */
export type ChunkTone = "live" | "done" | "idle" | "failed" | "complete";

export type ChunkCell = {
  id: string;
  /** Position and width as a percentage of the file. */
  leftPct: number;
  widthPct: number;
  /** Share of this range already on disk, 0..1. */
  fill: number;
  tone: ChunkTone;
  segment: TaskSegment;
};

/** Protocols whose segments are byte ranges of the one output file. HLS/DASH
 * segments are playlist entries, BT has pieces, and SFTP runs one stream, so
 * none of them has ranges to lay out. */
export function hasByteRangeSegments(protocol: string): boolean {
  return isHttpLikeProtocol(protocol) || (isFtpSftpProtocol(protocol) && protocol !== "sftp");
}

const ACTIVE_TASK_STATUSES = new Set<TaskStatus>(["downloading", "retrying"]);

export function chunkTone(segment: TaskSegment["status"], task: TaskStatus): ChunkTone {
  if (segment === "failed") return "failed";
  if (task === "completed") return "complete";
  // A paused or queued task keeps its ranges but nothing is moving: DESIGN.md
  // gives inactive states desaturated gray, not the accent.
  if (!ACTIVE_TASK_STATUSES.has(task)) return "idle";
  if (segment === "downloading") return "live";
  return segment === "completed" ? "done" : "idle";
}

/**
 * Lay the segments out along the file, or return null when a map would not be
 * truthful: fewer than two ranges (a single range is just the progress bar),
 * an unknown size, per-file segments (Metalink), or ranges outside the file.
 */
export function chunkMapCells(
  segments: readonly TaskSegment[],
  totalSize: number,
  taskStatus: TaskStatus,
): ChunkCell[] | null {
  if (segments.length < 2 || !(totalSize > 0)) return null;
  const cells: ChunkCell[] = [];
  for (const segment of segments) {
    if (segment.fileId !== null) return null;
    const { rangeStart, rangeEnd } = segment;
    if (!(rangeStart >= 0) || !(rangeEnd >= rangeStart) || rangeEnd >= totalSize) return null;
    const length = rangeEnd - rangeStart + 1;
    const written = segment.status === "completed" ? length : segment.downloadedUntil - rangeStart;
    cells.push({
      id: segment.id,
      leftPct: (rangeStart / totalSize) * 100,
      widthPct: (length / totalSize) * 100,
      fill: Math.min(1, Math.max(0, written / length)),
      tone: chunkTone(segment.status, taskStatus),
      segment,
    });
  }
  return cells.sort((a, b) => a.segment.rangeStart - b.segment.rangeStart);
}

export function chunkCounts(segments: readonly TaskSegment[]) {
  return {
    total: segments.length,
    completed: segments.filter((segment) => segment.status === "completed").length,
    active: segments.filter((segment) => segment.status === "downloading").length,
    failed: segments.filter((segment) => segment.status === "failed").length,
  };
}

/**
 * Carries the chunk strip between segment polls. Segment rows arrive every
 * couple of seconds while the task's byte total ticks every 250 ms, so a strip
 * drawn from the last poll would trail the percent beside it. The bytes that
 * arrived since the poll are spread over the ranges being written, weighted by
 * each connection's speed and capped at each range's size. Only the live
 * ranges grow, and the next poll replaces the estimate with engine truth.
 */
export function interpolateChunkCells(cells: readonly ChunkCell[], downloadedBytes: number): ChunkCell[] {
  const lengths = cells.map((cell) => cell.segment.rangeEnd - cell.segment.rangeStart + 1);
  const written = cells.reduce((sum, cell, index) => sum + cell.fill * lengths[index], 0);
  let extra = downloadedBytes - written;
  const live = cells
    .map((cell, index) => ({ cell, index }))
    .filter(({ cell }) => cell.tone === "live" && cell.fill < 1);
  if (!(extra > 0) || live.length === 0) return [...cells];

  const totalSpeed = live.reduce((sum, { cell }) => sum + Math.max(0, cell.segment.speedBps), 0);
  const fills = cells.map((cell) => cell.fill);
  for (const { cell, index } of live) {
    if (extra <= 0) break;
    const share = totalSpeed > 0 ? Math.max(0, cell.segment.speedBps) / totalSpeed : 1 / live.length;
    const room = (1 - cell.fill) * lengths[index];
    const add = Math.min(room, (downloadedBytes - written) * share, extra);
    fills[index] = cell.fill + add / lengths[index];
    extra -= add;
  }
  return cells.map((cell, index) => (fills[index] === cell.fill ? cell : { ...cell, fill: fills[index] }));
}
