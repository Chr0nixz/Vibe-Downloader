import { cn } from "@/lib/utils";

interface ProgressBarProps {
  value: number;
  label: string;
  active?: boolean;
  /** When false, skip transform transitions during frequent progress updates. */
  smooth?: boolean;
  tone?: "primary" | "success" | "danger" | "neutral";
  className?: string;
  trackClassName?: string;
}

const toneClass: Record<NonNullable<ProgressBarProps["tone"]>, string> = {
  primary: "bg-accent-primary",
  success: "bg-status-success",
  danger: "bg-status-danger",
  neutral: "bg-border-subtle",
};

export function ProgressBar({
  value,
  label,
  active = true,
  smooth = false,
  tone = "primary",
  className,
  trackClassName,
}: ProgressBarProps) {
  const clamped = Math.min(1, Math.max(0, value));
  const percent = Math.round(clamped * 100);

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${percent}%`}
      className={cn(
        "relative h-1 overflow-hidden rounded-full bg-surface-root",
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
          active ? toneClass[tone] : toneClass.neutral,
        )}
        style={{ transform: `scaleX(${clamped})` }}
      />
    </div>
  );
}
