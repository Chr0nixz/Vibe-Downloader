//! ChunkMap: the overview's progress bar drawn as the byte ranges the engine is
//! actually filling. Each range sits at its real offset in the file; its fill is
//! the part already on disk. Only a connection that is writing right now wears
//! the energy accent, so the strip is quiet unless the engine is busy.

import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { TaskStatus } from "@/generated/bindings";
import { type ChunkCell, type ChunkTone, chunkCounts } from "@/lib/chunk-map";
import { cn, formatBytes } from "@/lib/utils";
import type { TaskSegment } from "@/types/task-segment";

const TONE_FILL: Record<ChunkTone, string> = {
  live: "bg-accent-primary",
  done: "bg-accent-primary",
  idle: "bg-text-muted/45",
  failed: "bg-status-danger",
  complete: "bg-status-success",
};

export function ChunkMap({
  cells,
  segments,
  percent,
  taskStatus,
}: {
  cells: ChunkCell[];
  segments: readonly TaskSegment[];
  /** Whole-task progress, 0..100, for the progressbar value. */
  percent: number;
  taskStatus: TaskStatus;
}) {
  const { t } = useTranslation();
  const summary = chunkSummaryForStatus(segments, taskStatus, t);

  return (
    <div
      role="progressbar"
      aria-label={t("taskDetails.chunkMapAria")}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${percent}% · ${summary}`}
      className="relative h-2.5 overflow-hidden rounded-full bg-surface-track"
    >
      {cells.map((cell) => {
        const length = cell.segment.rangeEnd - cell.segment.rangeStart + 1;
        const tooltip = t("taskDetails.chunkTooltip", {
          range: `${formatBytes(cell.segment.rangeStart)} – ${formatBytes(cell.segment.rangeEnd + 1)}`,
          percent: `${Math.round(cell.fill * 100)}%`,
          retries: cell.segment.retryCount,
        });
        return (
          <div
            key={cell.id}
            title={`${tooltip} · ${formatBytes(length)}`}
            // The 1px seam in the panel colour is what makes adjacent ranges
            // read as separate connections rather than one bar.
            className="absolute inset-y-0 overflow-hidden shadow-[inset_-1px_0_0_var(--surface-base)]"
            style={{ left: `${cell.leftPct}%`, width: `${cell.widthPct}%` }}
          >
            <div
              aria-hidden
              className={cn(
                "h-full w-full origin-left transition-transform duration-ui ease-out motion-reduce:transition-none",
                taskStatus === "retrying" && (cell.tone === "live" || cell.tone === "done")
                  ? "bg-status-warning"
                  : TONE_FILL[cell.tone],
              )}
              style={{ transform: `scaleX(${cell.fill})` }}
            />
            {cell.tone === "live" && cell.fill < 1 ? (
              // The write head: a bright edge where this connection is
              // writing. Moved by transform so progress ticks never relayout.
              <div
                aria-hidden
                className="absolute inset-0 border-r-2 border-text-primary/70 transition-transform duration-ui ease-out motion-reduce:transition-none"
                style={{ transform: `translateX(${(cell.fill - 1) * 100}%)` }}
              />
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

/** The map's text equivalent (DESIGN.md: the chunk map needs a textual
 * summary), with the way into the per-range table. Rendered under the
 * percent/bytes line so the numbers stay next to the strip they describe. */
export function ChunkMapSummary({
  segments,
  taskStatus,
  onViewRanges,
}: {
  segments: readonly TaskSegment[];
  taskStatus: TaskStatus;
  onViewRanges: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between gap-2 px-1">
      <p className="min-w-0 text-xs leading-4 text-text-muted">{chunkSummaryForStatus(segments, taskStatus, t)}</p>
      <Button type="button" variant="ghost" size="sm" className="h-7 shrink-0 px-2 text-xs" onClick={onViewRanges}>
        {t("taskDetails.chunkMapViewRanges")}
      </Button>
    </div>
  );
}

function chunkSummaryForStatus(segments: readonly TaskSegment[], status: TaskStatus, t: TFunction): string {
  const counts = chunkCounts(segments);
  if ((status === "downloading" || status === "retrying") && counts.total > 0 && counts.completed === counts.total) {
    return t(`task.status.${status}`);
  }
  return t("taskDetails.chunksSummary", counts);
}
