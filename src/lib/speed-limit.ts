/**
 * Shared speed-limit input helpers. Both the global speed-limit panel in the
 * status bar and the per-task speed-limit control in TaskDetails use the same
 * amount + unit model so users learn one input pattern across the app.
 */

/** Preset caps offered by the global speed-limit panel and the command palette.
 * `null` means unlimited. Kept here so the two surfaces cannot drift apart. */
export const SPEED_LIMIT_PRESETS = [
  { id: "unlimited", label: "Unlimited", value: null },
  { id: "512k", label: "512 KB/s", value: 512 * 1024 },
  { id: "1m", label: "1 MB/s", value: 1024 * 1024 },
  { id: "5m", label: "5 MB/s", value: 5 * 1024 * 1024 },
  { id: "10m", label: "10 MB/s", value: 10 * 1024 * 1024 },
] as const;

export const SPEED_LIMIT_UNITS = [
  { value: "1", label: "B/s" },
  { value: "1024", label: "KB/s" },
  { value: "1048576", label: "MB/s" },
] as const;

export interface SpeedLimitInput {
  amount: string;
  unit: string;
}

export function speedLimitInputFromBytes(value: string | null | undefined): SpeedLimitInput {
  const bytes = Number(value ?? 0);
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return { amount: "", unit: "1048576" };
  }
  const unit =
    bytes >= 1024 * 1024 && bytes % (1024 * 1024) === 0 ? 1024 * 1024 : bytes >= 1024 && bytes % 1024 === 0 ? 1024 : 1;
  return {
    amount: String(bytes / unit),
    unit: String(unit),
  };
}

/**
 * Convert an amount + unit back to bytes/sec.
 * - Returns `null` when the amount is blank (means "unlimited").
 * - Returns `undefined` when the amount is present but invalid (caller shows error).
 */
export function speedLimitBytesFromInput(amount: string, unit: string): number | null | undefined {
  if (amount.trim() === "") return null;
  const parsedAmount = Number(amount);
  const parsedUnit = Number(unit);
  if (!Number.isFinite(parsedAmount) || !Number.isFinite(parsedUnit) || parsedAmount <= 0 || parsedUnit <= 0) {
    return undefined;
  }
  return Math.round(parsedAmount * parsedUnit);
}
