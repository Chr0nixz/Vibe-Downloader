import { describe, expect, it, vi } from "vitest";

import { scrollChildWithinContainer, stickyStartOffset } from "./scroll-within";

function rect(top: number, height: number): DOMRect {
  return {
    x: 0,
    y: top,
    top,
    left: 0,
    right: 100,
    bottom: top + height,
    width: 100,
    height,
    toJSON: () => ({}),
  };
}

function mockBox(top: number, height: number, scrollTop = 0) {
  const element = document.createElement("div");
  Object.defineProperty(element, "scrollTop", { value: scrollTop, writable: true });
  const scrollTo = vi.fn();
  element.scrollTo = scrollTo;
  vi.spyOn(element, "getBoundingClientRect").mockReturnValue(rect(top, height));
  return { element, scrollTo };
}

describe("scrollChildWithinContainer", () => {
  it("scrolls only the given container to align the child at start", () => {
    const { element: container, scrollTo } = mockBox(100, 400, 20);
    const { element } = mockBox(500, 80);
    scrollChildWithinContainer(container, element, { block: "start" });
    expect(scrollTo).toHaveBeenCalledWith({ top: 420, behavior: "auto" });
  });

  it("subtracts a sticky header offset for start alignment", () => {
    const { element: container, scrollTo } = mockBox(100, 400, 20);
    const { element } = mockBox(500, 80);
    scrollChildWithinContainer(container, element, { block: "start", offsetTop: 112 });
    expect(scrollTo).toHaveBeenCalledWith({ top: 308, behavior: "auto" });
  });

  it("centers the child inside the container", () => {
    const { element: container, scrollTo } = mockBox(100, 400, 20);
    const { element } = mockBox(500, 80);
    scrollChildWithinContainer(container, element, { block: "center", behavior: "smooth" });
    expect(scrollTo).toHaveBeenCalledWith({ top: 260, behavior: "smooth" });
  });

  it("does not scroll above the container origin", () => {
    const { element: container, scrollTo } = mockBox(100, 400, 0);
    const { element } = mockBox(50, 40);
    scrollChildWithinContainer(container, element, { block: "start" });
    expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "auto" });
  });
});

describe("stickyStartOffset", () => {
  it("reads the direct child nav height", () => {
    const container = document.createElement("div");
    const nav = document.createElement("nav");
    Object.defineProperty(nav, "offsetHeight", { value: 96 });
    container.append(nav);
    expect(stickyStartOffset(container)).toBe(96);
  });

  it("returns 0 when the sticky nav is missing", () => {
    expect(stickyStartOffset(document.createElement("div"))).toBe(0);
  });
});
