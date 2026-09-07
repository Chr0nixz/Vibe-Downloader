import { describe, expect, it, vi } from "vitest";

import { handleMenuKeyDown } from "./menu-keyboard";

function makeMenu(itemCount: number, disabledIndex?: number) {
  const menu = document.createElement("div");
  menu.setAttribute("role", "menu");
  for (let i = 0; i < itemCount; i += 1) {
    const item = document.createElement("button");
    item.setAttribute("role", "menuitem");
    item.textContent = `item-${i}`;
    if (i === disabledIndex) item.disabled = true;
    menu.append(item);
  }
  document.body.append(menu);
  return menu;
}

describe("handleMenuKeyDown", () => {
  it("moves focus with arrows and Home/End, skipping disabled items", () => {
    const menu = makeMenu(4, 1);
    const items = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')];
    items[0].focus();

    const preventDefault = vi.fn();
    handleMenuKeyDown({ key: "ArrowDown", currentTarget: menu, target: items[0], preventDefault });
    expect(preventDefault).toHaveBeenCalled();
    expect(document.activeElement).toBe(items[1]);

    handleMenuKeyDown({ key: "End", currentTarget: menu, target: items[1], preventDefault });
    expect(document.activeElement).toBe(items[2]);

    handleMenuKeyDown({ key: "Home", currentTarget: menu, target: items[2], preventDefault });
    expect(document.activeElement).toBe(items[0]);
  });
});
