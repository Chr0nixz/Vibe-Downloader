import { describe, expect, it } from "vitest";

import { TASK_ROW_COMPACT_ESTIMATED_SIZE, TASK_ROW_ESTIMATED_SIZE, taskRowEstimateFor } from "./task-layout";

describe("task list layout constants", () => {
  it("keeps the virtual row estimate aligned with the comfortable default row", () => {
    expect(TASK_ROW_ESTIMATED_SIZE).toBeGreaterThanOrEqual(84);
    expect(TASK_ROW_ESTIMATED_SIZE).toBeLessThanOrEqual(100);
  });

  it("keeps the compact estimate near a 60px two-line row", () => {
    expect(TASK_ROW_COMPACT_ESTIMATED_SIZE).toBeGreaterThanOrEqual(52);
    expect(TASK_ROW_COMPACT_ESTIMATED_SIZE).toBeLessThanOrEqual(68);
  });

  it("maps density to its estimate, and compact stays shorter than comfortable", () => {
    expect(taskRowEstimateFor("comfortable")).toBe(TASK_ROW_ESTIMATED_SIZE);
    expect(taskRowEstimateFor("compact")).toBe(TASK_ROW_COMPACT_ESTIMATED_SIZE);
    expect(taskRowEstimateFor("compact")).toBeLessThan(taskRowEstimateFor("comfortable"));
  });
});
