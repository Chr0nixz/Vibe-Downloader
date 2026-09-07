import type { RowDensity } from "@/stores/task-ui-store";

/** Virtualizer size estimates, used only until `measureElement` reports the real
 * height of each rendered row. Undershooting is safer than overshooting: an
 * over-tall estimate pads the scroll area with blank space until the first
 * measurement pass corrects it, which reads as a jump at the list bottom. */
export const TASK_ROW_ESTIMATED_SIZE = 96;
export const TASK_ROW_COMPACT_ESTIMATED_SIZE = 60;

export function taskRowEstimateFor(density: RowDensity): number {
  return density === "compact" ? TASK_ROW_COMPACT_ESTIMATED_SIZE : TASK_ROW_ESTIMATED_SIZE;
}
