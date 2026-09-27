import { useEffect, useRef, useState } from "react";

import { useVisibilityGatedPoll } from "@/hooks/use-visibility-gated-poll";
import { hasByteRangeSegments } from "@/lib/chunk-map";
import { listSegments } from "@/lib/tauri";
import { useTaskDataStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";

/** Same cadence as the details panel's segment poll, so the row strip and the
 * panel's chunk map never disagree for long. */
const ROW_SEGMENT_REFRESH_MS = 2_000;

/** Cap on rows polled per tick: the visible window plus overscan is rarely
 * larger, and a tall monitor must not turn one tick into dozens of IPC calls. */
const MAX_POLLED_ROWS = 16;

/** HTTP caps at 8 ranges and FTP at 4; one page covers any row. */
const SEGMENT_PAGE_SIZE = 64;

const EMPTY: ReadonlyMap<string, readonly TaskSegment[]> = new Map();

const LIVE_STATUSES = new Set<Task["status"]>(["downloading", "retrying"]);
const RANGE_STATUSES = new Set<Task["status"]>(["downloading", "retrying", "paused", "waiting_network"]);

/** Rows that can draw their bar as byte ranges: an unfinished single-file
 * transfer with a known size on a protocol whose segments are file ranges. */
export function rowCanShowRanges(task: Task): boolean {
  return (
    RANGE_STATUSES.has(task.status) &&
    task.supportsParallel &&
    task.totalSize > 0 &&
    task.downloadedBytes < task.totalSize &&
    hasByteRangeSegments(task.protocol)
  );
}

function sameSegments(a: readonly TaskSegment[], b: readonly TaskSegment[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (segment, index) =>
        segment.id === b[index].id &&
        segment.status === b[index].status &&
        segment.downloadedUntil === b[index].downloadedUntil &&
        segment.speedBps === b[index].speedBps,
    )
  );
}

/**
 * Byte-range segments for the visible rows that can draw them.
 *
 * One shared poll per list (like `useQueueReasons`) keeps segment IPC off the
 * per-row render path: only rows on screen are asked, live ones every tick,
 * paused ones once per status change. Rows with fewer than two ranges are
 * remembered and skipped, since a single range is just the plain bar.
 *
 * The returned Map and each segment array keep their identity while nothing
 * changed, so memoized rows do not re-render on an idle poll.
 */
export function useRowSegments(visibleTaskIds: readonly string[]): ReadonlyMap<string, readonly TaskSegment[]> {
  // A joined key (not an array) so the selector compares by value and
  // progress ticks never invalidate it; status is part of the key because a
  // pause or resume can re-plan the ranges.
  const eligibleKey = useTaskDataStore((state) => {
    const parts: string[] = [];
    for (const id of visibleTaskIds) {
      const task = state.taskById[id];
      if (!task || !rowCanShowRanges(task)) continue;
      parts.push(`${id}\u0001${task.status}`);
      if (parts.length >= MAX_POLLED_ROWS) break;
    }
    return parts.join("\u0000");
  });
  const [snapshot, setSnapshot] = useState<ReadonlyMap<string, readonly TaskSegment[]>>(EMPTY);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  // `${id}\u0001${status}` entries known to have fewer than two ranges.
  const singleRangeRef = useRef(new Set<string>());
  const hasLive = eligibleKey
    .split("\u0000")
    .some((entry) => LIVE_STATUSES.has(entry.split("\u0001")[1] as Task["status"]));

  useVisibilityGatedPoll(
    async (isStale) => {
      const entries = eligibleKey
        .split("\u0000")
        .filter(Boolean)
        .map((entry) => {
          const [id, status] = entry.split("\u0001");
          return { entry, id, live: LIVE_STATUSES.has(status as Task["status"]) };
        })
        // Paused ranges do not move, so they are fetched once and kept.
        .filter(({ entry, id, live }) => !singleRangeRef.current.has(entry) && (live || !snapshotRef.current.has(id)));
      if (entries.length === 0) return;

      const results = await Promise.all(
        entries.map(async ({ entry, id }) => {
          try {
            return { entry, id, segments: await listSegments(id, 0, SEGMENT_PAGE_SIZE) };
          } catch {
            // The row keeps its plain bar; a failed lookup is not an error the
            // user needs to see in the list.
            return { entry, id, segments: null };
          }
        }),
      );
      if (isStale()) return;

      setSnapshot((previous) => {
        let changed = false;
        const next = new Map(previous);
        for (const { entry, id, segments } of results) {
          if (!segments || segments.length < 2) {
            if (segments) singleRangeRef.current.add(entry);
            changed = next.delete(id) || changed;
            continue;
          }
          const prior = next.get(id);
          if (prior && sameSegments(prior, segments)) continue;
          next.set(id, segments);
          changed = true;
        }
        return changed ? next : previous;
      });
    },
    ROW_SEGMENT_REFRESH_MS,
    { enabled: Boolean(eligibleKey), poll: hasLive, reloadKey: eligibleKey },
  );

  // Rows that scrolled away or finished drop their ranges so the map (and the
  // rows reading it) cannot show stale evidence when they come back.
  useEffect(() => {
    const keep = new Set(eligibleKey.split("\u0000").map((entry) => entry.split("\u0001")[0]));
    setSnapshot((previous) => {
      let changed = false;
      const next = new Map(previous);
      for (const id of previous.keys()) {
        if (!keep.has(id)) {
          next.delete(id);
          changed = true;
        }
      }
      return changed ? next : previous;
    });
  }, [eligibleKey]);

  return snapshot;
}
