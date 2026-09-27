import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useState } from "react";
import { describe, expect, it } from "vitest";

import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { isOverlayKey } from "./shell-keys";

/** Mirrors the shell: a window listener that closes the details panel on Esc. */
function Harness({ initialDialogOpen }: { initialDialogOpen: boolean }) {
  const [detailOpen, setDetailOpen] = useState(true);
  const [dialogOpen, setDialogOpen] = useState(initialDialogOpen);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !isOverlayKey(event)) setDetailOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <>
      <button type="button">task row</button>
      {detailOpen ? <aside aria-label="details">details</aside> : null}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent>
          <DialogTitle>Restart download</DialogTitle>
          <DialogDescription>Discards 84.9 MB.</DialogDescription>
          <button type="button">Cancel</button>
        </DialogContent>
      </Dialog>
    </>
  );
}

describe("isOverlayKey", () => {
  it("spends Esc on the confirm dialog instead of also closing the details panel", async () => {
    render(<Harness initialDialogOpen />);

    fireEvent.keyDown(screen.getByRole("button", { name: "Cancel" }), { key: "Escape" });

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("complementary", { name: "details" })).toBeInTheDocument();
  });

  it("lets Esc on the list surface close the details panel", () => {
    render(<Harness initialDialogOpen={false} />);

    fireEvent.keyDown(screen.getByRole("button", { name: "task row" }), { key: "Escape" });

    expect(screen.queryByRole("complementary", { name: "details" })).not.toBeInTheDocument();
  });

  it("treats any key pressed inside a dialog as the dialog's own", () => {
    render(<Harness initialDialogOpen />);
    const inside = screen.getByRole("button", { name: "Cancel" });

    let owned: boolean | null = null;
    const listener = (event: KeyboardEvent) => {
      owned = isOverlayKey(event);
    };
    window.addEventListener("keydown", listener);
    fireEvent.keyDown(inside, { key: "Delete" });
    window.removeEventListener("keydown", listener);

    expect(owned).toBe(true);
  });
});
