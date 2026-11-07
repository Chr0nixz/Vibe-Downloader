import { describe, expect, it } from "vitest";
import { selectionSuccessor } from "@/components/shell/selection-successor";

describe("selectionSuccessor", () => {
  const order = ["a", "b", "c", "d"];

  it("moves to the next row after the removed selection", () => {
    expect(selectionSuccessor(order, "b", new Set(["b"]))).toBe("c");
  });

  it("falls back to the previous row at the end of the list", () => {
    expect(selectionSuccessor(order, "d", new Set(["d"]))).toBe("c");
  });

  it("skips rows removed in the same batch and rows already pending delete", () => {
    expect(selectionSuccessor(order, "a", new Set(["a", "b"]), new Set(["c"]))).toBe("d");
    expect(selectionSuccessor(order, "c", new Set(["c", "d"]), new Set(["b"]))).toBe("a");
  });

  it("returns null when nothing survives or the selection is not listed", () => {
    expect(selectionSuccessor(order, "b", new Set(order))).toBeNull();
    expect(selectionSuccessor(order, "z", new Set(["z"]))).toBeNull();
  });
});
