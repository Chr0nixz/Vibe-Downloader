import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { type ReactNode, useEffect, useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import type { Platform } from "@/lib/platform";
import { isGlobalPasteShortcut, isOverlayKey, isTextInputTarget } from "./shell-keys";

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

describe("global paste shortcut", () => {
  it("accepts Ctrl+V on the task surface and Command+V on macOS", () => {
    expect(isGlobalPasteShortcut(new KeyboardEvent("keydown", { key: "v", ctrlKey: true }), "windows")).toBe(true);
    expect(isGlobalPasteShortcut(new KeyboardEvent("keydown", { key: "V", metaKey: true }), "macos")).toBe(true);
    expect(isGlobalPasteShortcut(new KeyboardEvent("keydown", { key: "v", ctrlKey: true }), "macos")).toBe(false);
  });

  it("does not claim modified variants that belong to other commands", () => {
    expect(
      isGlobalPasteShortcut(new KeyboardEvent("keydown", { key: "v", ctrlKey: true, shiftKey: true }), "windows"),
    ).toBe(false);
    expect(
      isGlobalPasteShortcut(new KeyboardEvent("keydown", { key: "v", ctrlKey: true, altKey: true }), "windows"),
    ).toBe(false);
  });

  it("recognizes text editors while leaving non-text controls available", () => {
    const input = document.createElement("input");
    input.type = "text";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    const textarea = document.createElement("textarea");
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");

    expect(isTextInputTarget(input)).toBe(true);
    expect(isTextInputTarget(textarea)).toBe(true);
    expect(isTextInputTarget(editor)).toBe(true);
    expect(isTextInputTarget(checkbox)).toBe(false);
    expect(isTextInputTarget(document.createElement("button"))).toBe(false);
  });
});

function PasteHarness({
  children,
  onPaste,
  platform = "windows",
}: {
  children: ReactNode;
  onPaste: () => void;
  platform?: Platform;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isGlobalPasteShortcut(event, platform)) return;
      event.preventDefault();
      onPaste();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onPaste, platform]);
  return children;
}

function pasteKey(target: Element, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", {
    key: "v",
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
    ...options,
  });
  fireEvent(target, event);
  return event;
}

describe("global paste events", () => {
  it.each(["windows", "linux", "macos"] as const)("opens from the task surface on %s", (platform) => {
    const onPaste = vi.fn();
    render(
      <PasteHarness onPaste={onPaste} platform={platform}>
        <button type="button">task row</button>
      </PasteHarness>,
    );
    const event = pasteKey(screen.getByRole("button"), {
      ctrlKey: platform !== "macos",
      metaKey: platform === "macos",
    });
    expect(event.defaultPrevented).toBe(true);
    expect(onPaste).toHaveBeenCalledTimes(1);
  });

  it.each(["text", "search", "url", "password", "email", "number"])("preserves native paste in a %s input", (type) => {
    const onPaste = vi.fn();
    render(
      <PasteHarness onPaste={onPaste}>
        <input type={type} aria-label="entry" />
      </PasteHarness>,
    );
    const entry = screen.getByLabelText("entry");
    entry.focus();
    expect(pasteKey(entry).defaultPrevented).toBe(false);
    expect(onPaste).not.toHaveBeenCalled();
  });

  it("preserves native paste in a textarea, select, and nested contenteditable", () => {
    const onPaste = vi.fn();
    render(
      <PasteHarness onPaste={onPaste}>
        <textarea aria-label="notes" />
        <select aria-label="choice">
          <option>one</option>
        </select>
        <div contentEditable suppressContentEditableWarning>
          <span>editable child</span>
        </div>
      </PasteHarness>,
    );
    for (const target of [
      screen.getByLabelText("notes"),
      screen.getByLabelText("choice"),
      screen.getByText("editable child"),
    ]) {
      expect(pasteKey(target).defaultPrevented).toBe(false);
    }
    expect(onPaste).not.toHaveBeenCalled();
  });

  it.each(["dialog", "alertdialog", "menu"])("leaves keys inside a %s to that overlay", (role) => {
    const onPaste = vi.fn();
    render(
      <PasteHarness onPaste={onPaste}>
        <button type="button">task row</button>
      </PasteHarness>,
    );
    const target = screen.getByRole("button");
    target.parentElement?.setAttribute("role", role);
    expect(pasteKey(target).defaultPrevented).toBe(false);
    expect(onPaste).not.toHaveBeenCalled();
  });

  it("ignores composing, repeated, consumed, and unrelated chords", () => {
    const onPaste = vi.fn();
    render(
      <PasteHarness onPaste={onPaste}>
        <button type="button">task row</button>
      </PasteHarness>,
    );
    const target = screen.getByRole("button");
    for (const options of [
      { isComposing: true },
      { repeat: true },
      { shiftKey: true },
      { altKey: true },
      { metaKey: true },
      { ctrlKey: false },
      { key: "c" },
    ]) {
      expect(pasteKey(target, options).defaultPrevented).toBe(false);
    }
    const consumed = new KeyboardEvent("keydown", { key: "v", ctrlKey: true, bubbles: true, cancelable: true });
    consumed.preventDefault();
    fireEvent(target, consumed);
    expect(onPaste).not.toHaveBeenCalled();
  });
});
