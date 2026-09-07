type ScrollWithinBlock = "start" | "center";

type ScrollWithinOptions = {
  behavior?: ScrollBehavior;
  block?: ScrollWithinBlock;
  /** Distance from the container’s top edge when `block` is `start` (sticky header). */
  offsetTop?: number;
};

/**
 * Scroll `element` inside `container` only.
 * Native `scrollIntoView` also moves overflow:hidden ancestors (html/body/#root),
 * which jumps the whole app chrome when the target sits near the bottom.
 */
export function scrollChildWithinContainer(
  container: HTMLElement,
  element: HTMLElement,
  options: ScrollWithinOptions = {},
): void {
  const behavior = options.behavior ?? "auto";
  const block = options.block ?? "start";
  const offsetTop = options.offsetTop ?? 0;
  const containerRect = container.getBoundingClientRect();
  const elementRect = element.getBoundingClientRect();
  const alignment = block === "center" ? containerRect.height / 2 - elementRect.height / 2 : offsetTop;
  const top = container.scrollTop + (elementRect.top - containerRect.top) - alignment;
  container.scrollTo({ top: Math.max(0, top), behavior });
}

export function stickyStartOffset(container: HTMLElement): number {
  const nav = container.querySelector(":scope > nav");
  return nav instanceof HTMLElement ? nav.offsetHeight : 0;
}
