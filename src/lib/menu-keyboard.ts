/** Arrow/Home/End movement for APG-style menus that are not Radix Menu. */
export function handleMenuKeyDown(event: {
  key: string;
  currentTarget: HTMLElement;
  target: EventTarget | null;
  preventDefault: () => void;
}): void {
  const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)'));
  if (items.length === 0) return;

  const index = items.indexOf(event.target as HTMLElement);
  let next = index;
  if (event.key === "ArrowDown") {
    next = index < 0 ? 0 : (index + 1) % items.length;
  } else if (event.key === "ArrowUp") {
    next = index < 0 ? items.length - 1 : (index - 1 + items.length) % items.length;
  } else if (event.key === "Home") {
    next = 0;
  } else if (event.key === "End") {
    next = items.length - 1;
  } else {
    return;
  }

  event.preventDefault();
  items[next]?.focus();
}
