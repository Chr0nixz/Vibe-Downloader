import { describe, expect, it } from "vitest";

import { FLOATING_LIVE_INTERVAL_MS, shouldAnnounceFloatingStatus } from "./floating-live-status";

describe("shouldAnnounceFloatingStatus", () => {
  it("announces the first sample and idle transitions", () => {
    const first = shouldAnnounceFloatingStatus(true, 0, 0, null);
    expect(first.announce).toBe(true);

    const stillIdle = shouldAnnounceFloatingStatus(true, 0, 200, first.next);
    expect(stillIdle.announce).toBe(false);

    const active = shouldAnnounceFloatingStatus(false, 12, 400, stillIdle.next);
    expect(active.announce).toBe(true);
  });

  it("announces on 10% buckets or the idle timeout, not every tick", () => {
    const start = shouldAnnounceFloatingStatus(false, 12, 0, null);
    const sameBucket = shouldAnnounceFloatingStatus(false, 18, 400, start.next);
    expect(sameBucket.announce).toBe(false);

    const nextBucket = shouldAnnounceFloatingStatus(false, 21, 800, sameBucket.next);
    expect(nextBucket.announce).toBe(true);

    const timed = shouldAnnounceFloatingStatus(false, 24, FLOATING_LIVE_INTERVAL_MS + 800, nextBucket.next);
    expect(timed.announce).toBe(true);
  });
});
