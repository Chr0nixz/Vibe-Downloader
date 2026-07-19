import { describe, expect, it } from "vitest";

import { accumulateQueueChanged, createQueueChangedAccumulator, takeQueueFlushPlan } from "./use-task-events";

describe("queue-changed debounce accumulator (ARC-09)", () => {
  it("merges changed_task_ids across rapid events", () => {
    const state = createQueueChangedAccumulator();
    accumulateQueueChanged(state, { changed_task_ids: ["a"] });
    accumulateQueueChanged(state, { changed_task_ids: ["b"] });
    expect(takeQueueFlushPlan(state)).toEqual({ kind: "incremental", ids: ["a", "b"] });
  });

  it("dedupes repeated task ids in the debounce window", () => {
    const state = createQueueChangedAccumulator();
    accumulateQueueChanged(state, { changed_task_ids: ["a", "b"] });
    accumulateQueueChanged(state, { changed_task_ids: ["b", "a"] });
    const plan = takeQueueFlushPlan(state);
    expect(plan.kind).toBe("incremental");
    if (plan.kind === "incremental") {
      expect(plan.ids.sort()).toEqual(["a", "b"]);
    }
  });

  it("escalates to full refresh when any event has null changed_task_ids", () => {
    const state = createQueueChangedAccumulator();
    accumulateQueueChanged(state, { changed_task_ids: ["a"] });
    accumulateQueueChanged(state, null);
    accumulateQueueChanged(state, { changed_task_ids: ["b"] });
    expect(takeQueueFlushPlan(state)).toEqual({ kind: "full" });
  });

  it("escalates to full refresh when accumulated ids exceed 50", () => {
    const state = createQueueChangedAccumulator();
    accumulateQueueChanged(state, {
      changed_task_ids: Array.from({ length: 51 }, (_, i) => `t${i}`),
    });
    expect(takeQueueFlushPlan(state)).toEqual({ kind: "full" });
  });

  it("does not drop the first batch when a second event arrives within the window", () => {
    const state = createQueueChangedAccumulator();
    accumulateQueueChanged(state, { changed_task_ids: ["first"] });
    accumulateQueueChanged(state, { changed_task_ids: ["second"] });
    const plan = takeQueueFlushPlan(state);
    expect(plan).toEqual({ kind: "incremental", ids: ["first", "second"] });
    expect(takeQueueFlushPlan(state)).toEqual({ kind: "noop" });
  });
});
