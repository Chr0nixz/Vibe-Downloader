import { afterEach, describe, expect, it } from "vitest";

import {
  beginListLoad,
  bumpListQueryEpoch,
  createListLoadFlight,
  endListLoad,
  getListQueryEpoch,
  isCurrentListQueryEpoch,
  resetListQueryEpochForTests,
} from "@/lib/list-query-epoch";

afterEach(() => {
  resetListQueryEpochForTests();
});

describe("list-query-epoch", () => {
  it("ignores stale epochs after a newer replace bump", () => {
    const first = bumpListQueryEpoch();
    expect(isCurrentListQueryEpoch(first)).toBe(true);
    const second = bumpListQueryEpoch();
    expect(isCurrentListQueryEpoch(first)).toBe(false);
    expect(isCurrentListQueryEpoch(second)).toBe(true);
    expect(getListQueryEpoch()).toBe(second);
  });

  it("replays pending query after in-flight replace completes", () => {
    const flight = createListLoadFlight();
    const first = beginListLoad(flight, false);
    expect(first).toEqual({ kind: "start", epoch: 1, role: "replace" });

    const skipped = beginListLoad(flight, false);
    expect(skipped).toEqual({ kind: "skip" });
    expect(flight.pendingReload).toBe(true);
    expect(getListQueryEpoch()).toBe(2);

    expect(endListLoad(flight, "replace")).toBe(true);
    expect(flight.pendingReload).toBe(false);

    const replay = beginListLoad(flight, false);
    expect(replay.kind).toBe("start");
    if (replay.kind === "start") {
      expect(replay.epoch).toBe(3);
      expect(replay.role).toBe("replace");
    }
  });

  it("does not apply append after query epoch bump", () => {
    const flight = createListLoadFlight();
    const append = beginListLoad(flight, true);
    expect(append).toEqual({ kind: "start", epoch: 0, role: "append" });

    bumpListQueryEpoch();
    expect(append.kind).toBe("start");
    if (append.kind === "start") {
      expect(isCurrentListQueryEpoch(append.epoch)).toBe(false);
    }

    expect(endListLoad(flight, "append")).toBe(false);
  });

  it("skips append while replace is in flight and queues reload when replace arrives mid-append", () => {
    const flight = createListLoadFlight();
    expect(beginListLoad(flight, true).kind).toBe("start");
    expect(beginListLoad(flight, true).kind).toBe("skip");

    // Replace bumps epoch; append still holds the flight until endListLoad.
    const midReplace = beginListLoad(flight, false);
    expect(midReplace.kind).toBe("start");
    expect(endListLoad(flight, "append")).toBe(false);
    if (midReplace.kind === "start") {
      expect(endListLoad(flight, "replace")).toBe(false);
    }
  });
});
