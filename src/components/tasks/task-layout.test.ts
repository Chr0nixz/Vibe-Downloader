import { describe, expect, it } from "vitest";

import { TASK_ROW_ESTIMATED_SIZE } from "./task-layout";

describe("task list layout constants", () => {
  it("keeps the virtual row estimate aligned with the compact default row", () => {
    expect(TASK_ROW_ESTIMATED_SIZE).toBeGreaterThanOrEqual(84);
    expect(TASK_ROW_ESTIMATED_SIZE).toBeLessThanOrEqual(100);
  });
});
