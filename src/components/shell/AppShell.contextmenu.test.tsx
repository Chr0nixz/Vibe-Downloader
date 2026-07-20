import { createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { describe, expect, it } from "vitest";

import { MenuItem, RegionContextMenu } from "@/components/ui/menu-item";

/**
 * UX-02: The production AppShell listener runs on bubble phase and skips
 * editing surfaces. These helpers mirror that contract so the test stays
 * focused on event ordering without mounting the full shell.
 */
function useNativeContextMenuSuppression() {
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      const target = event.target;
      if (target instanceof Element) {
        if (target.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]')) {
          return;
        }
      }
      event.preventDefault();
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);
}

function Harness({ children }: { children: React.ReactNode }) {
  useNativeContextMenuSuppression();
  return <>{children}</>;
}

describe("UX-02 native contextmenu suppression", () => {
  it("allows RegionContextMenu triggers to open with a real contextmenu event", async () => {
    render(
      <Harness>
        <RegionContextMenu items={<MenuItem label="Open details" onSelect={() => undefined} />}>
          <button type="button">task-row</button>
        </RegionContextMenu>
      </Harness>,
    );

    fireEvent.contextMenu(screen.getByRole("button", { name: "task-row" }));
    await waitFor(() => {
      expect(screen.getByRole("menuitem", { name: "Open details" })).toBeInTheDocument();
    });
  });

  it("suppresses the native menu on blank surfaces", () => {
    render(
      <Harness>
        <div data-testid="blank">blank</div>
      </Harness>,
    );

    const blank = screen.getByTestId("blank");
    const event = createEvent.contextMenu(blank);
    fireEvent(blank, event);
    expect(event.defaultPrevented).toBe(true);
  });

  it("does not suppress contextmenu inside text inputs", () => {
    render(
      <Harness>
        <input aria-label="url" defaultValue="https://example.com" />
      </Harness>,
    );

    const input = screen.getByLabelText("url");
    const event = createEvent.contextMenu(input);
    fireEvent(input, event);
    expect(event.defaultPrevented).toBe(false);
  });
});
