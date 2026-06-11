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
  primary:
    "bg-[linear-gradient(90deg,var(--accent-primary),var(--accent-energy))]",
  success: "bg-status-success",
  danger: "bg-status-danger",
  neutral: "bg-border-subtle",
};

const toneGlow: Record<NonNullable<ProgressBarProps["tone"]>, string> = {
  primary: "shadow-[0_0_8px_oklch(0.72_0.14_235_/_0.35)]",
  success: "shadow-[0_0_6px_oklch(0.72_0.14_150_/_0.3)]",
  danger: "shadow-[0_0_6px_oklch(0.68_0.18_25_/_0.3)]",
  neutral: "",
};

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

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent}
      aria-valuetext={`${percent}%`}
      className={cn(
        "relative overflow-hidden rounded-full bg-surface-root",
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
          active ? toneFill[tone] : toneFill.neutral,
          active && tone !== "neutral" && size === "lg" && toneGlow[tone],
          "completion-flash-progress",
        )}
        style={{ transform: `scaleX(${clamped})` }}
      />
    </div>
  );
}
