import type {
  SegmentStatus,
  TaskSegment as GeneratedTaskSegment,
} from "@/generated/bindings";
import { parseByteCount } from "@/types/task";

export type { SegmentStatus };

export type TaskSegment = Omit<
  GeneratedTaskSegment,
  "rangeStart" | "rangeEnd" | "downloadedUntil"
> & {
  rangeStart: number;
  rangeEnd: number;
  downloadedUntil: number;
};

export function normalizeTaskSegment(segment: GeneratedTaskSegment): TaskSegment {
  return {
    ...segment,
    rangeStart: parseByteCount(segment.rangeStart),
    rangeEnd: parseByteCount(segment.rangeEnd),
    downloadedUntil: parseByteCount(segment.downloadedUntil),
  };
}
