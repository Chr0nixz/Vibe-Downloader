import { cn } from "@/lib/utils";

interface ProgressBarProps {
  value: number;
  label: string;
  active?: boolean;
  /** When false, skip transform transitions during frequent progress updates. */
  smooth?: boolean;
  tone?: "primary" | "success" | "danger" | "neutral";
  /** Increase height for prominence; "default" keeps the slim row variant. */
  size?: "default" | "lg";
  className?: string;
  trackClassName?: string;
}

const sizeClass: Record<NonNullable<ProgressBarProps["size"]>, string> = {
  default: "h-1.5",
  lg: "h-2.5",
};

const toneFill: Record<NonNullable<ProgressBarProps["tone"]>, string> = {
  primary: "bg-accent-primary",
  success: "bg-status-success",
  danger: "bg-status-danger",
  neutral: "bg-border-subtle",
};

const toneGlow: Record<NonNullable<ProgressBarProps["tone"]>, string> = {
  primary: "shadow-[0_0_8px_color-mix(in_oklch,var(--accent-primary)_35%,transparent)]",
  success: "shadow-[0_0_6px_color-mix(in_oklch,var(--status-success)_30%,transparent)]",
  danger: "shadow-[0_0_6px_color-mix(in_oklch,var(--status-danger)_30%,transparent)]",
  neutral: "",
};

// Large active primary fills use the three-hue accent gradient (energy→primary→peak)
// instead of a flat solid. The gradient is part of the product's visual signature;
// slim row bars stay solid so the row doesn't get noisier at default density.
const lgGradientFill =
  "bg-[linear-gradient(to_top,var(--accent-energy)_0%,var(--accent-primary)_55%,var(--accent-peak)_100%)]";

export function ProgressBar({
  value,
  label,
  active = true,
  smooth = false,
  tone = "primary",
  size = "default",
  className,
  trackClassName,
}: ProgressBarProps) {
  const clamped = Math.min(1, Math.max(0, value));
  const percent = Math.round(clamped * 100);
  const useGradient = active && tone === "primary" && size === "lg";

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${percent}%`}
      className={cn(
        // Track now uses a dedicated --surface-track token so the bar is visible
        // against the page background instead of disappearing into it.
        "relative overflow-hidden rounded-full bg-surface-track",
        sizeClass[size],
        trackClassName,
        className,
      )}
    >
      <div
        aria-hidden
        className={cn(
          "absolute inset-y-0 left-0 w-full origin-left rounded-full",
          smooth
            ? "transition-transform duration-ui ease-out will-change-transform motion-reduce:transition-none"
            : "transition-none",
          useGradient ? lgGradientFill : active ? toneFill[tone] : toneFill.neutral,
          active && tone !== "neutral" && size === "lg" && !useGradient && toneGlow[tone],
          useGradient && "shadow-[0_0_8px_color-mix(in_oklch,var(--accent-primary)_40%,transparent)]",
        )}
        style={{ transform: `scaleX(${clamped})` }}
      />
    </div>
  );
}
