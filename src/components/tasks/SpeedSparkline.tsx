import { useMemo } from "react";
import type { TFunction } from "i18next";

import { cn, formatSpeed } from "@/lib/utils";
import type { SpeedSample } from "@/stores/speed-history-store";

interface SpeedTrend {
  label: string;
  tone: "muted" | "stable" | "warning";
}

export function describeSpeedTrend(
  samples: SpeedSample[],
  currentSpeedBps: number,
  t: TFunction,
): SpeedTrend {
  if (currentSpeedBps <= 0 || samples.length < 3) {
    return { label: t("taskDiagnostics.idle"), tone: "muted" };
  }

  const recent = samples.slice(-8).map((sample) => sample.speedBps);
  const previous = samples.slice(-16, -8).map((sample) => sample.speedBps);
  const recentAverage = average(recent);
  const previousAverage = previous.length > 0 ? average(previous) : recentAverage;
  const recentMin = Math.min(...recent);
  const recentMax = Math.max(...recent);

  if (
    recentMin === 0 ||
    recentMax > Math.max(1, recentMin) * 2.8 ||
    recentAverage < previousAverage * 0.65
  ) {
    return { label: t("taskDiagnostics.fluctuating"), tone: "warning" };
  }

  return { label: t("taskDiagnostics.stable"), tone: "stable" };
}

export function SpeedSparkline({
  samples,
  currentSpeedBps,
  label,
  className,
}: {
  samples: SpeedSample[];
  currentSpeedBps: number;
  label: string;
  className?: string;
}) {
  const points = useMemo(() => buildPoints(samples), [samples]);
  const hasData = samples.some((sample) => sample.speedBps > 0);

  return (
    <div
      className={cn(
        "flex h-12 min-w-36 items-center gap-3 rounded-md border border-border-panel bg-surface-root/55 px-3",
        className,
      )}
    >
      <svg
        viewBox="0 0 120 32"
        role="img"
        aria-label={label}
        className={cn(
          "h-8 min-w-0 flex-1 overflow-visible text-accent-energy",
          !hasData && "text-text-muted",
        )}
      >
        <polyline
          points={points}
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="2"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <span className="shrink-0 font-mono text-xs text-text-primary">
        {formatSpeed(currentSpeedBps)}
      </span>
    </div>
  );
}

function average(values: number[]): number {
  if (values.length === 0) return 0;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function buildPoints(samples: SpeedSample[]): string {
  const values = samples.length > 1 ? samples : [{ at: 0, speedBps: 0 }, ...samples];
  const visible = values.slice(-24);
  const max = Math.max(1, ...visible.map((sample) => sample.speedBps));
  const width = 120;
  const height = 32;
  const lastIndex = Math.max(1, visible.length - 1);

  return visible
    .map((sample, index) => {
      const x = (index / lastIndex) * width;
      const y = height - (sample.speedBps / max) * (height - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
