import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { isModalFocusActive } from "@/lib/modal-focus";
import { useToastStore } from "@/stores/toast-store";

function Harness({ open }: { open: boolean }) {
  return (
    <Dialog open={open}>
      <DialogContent>
        <DialogTitle>Title</DialogTitle>
        <DialogDescription>Body</DialogDescription>
      </DialogContent>
    </Dialog>
  );
}

describe("dialog modal-focus claim", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
  });

  afterEach(() => {
    cleanup();
    useToastStore.setState({ toasts: [] });
  });

  // Regression: the claim used to live in the DialogContent wrapper's mount
  // effect, so a dialog kept mounted while closed (NewDownloadDialog) held it
  // for the whole session and every success/info toast — Undo included — was
  // deferred forever.
  it("does not claim focus while a mounted dialog is closed", () => {
    render(<Harness open={false} />);
    expect(isModalFocusActive()).toBe(false);

    act(() => {
      useToastStore.getState().addToast({ tone: "success", title: "Deleted demo.zip" });
    });
    expect(useToastStore.getState().toasts.map((toast) => toast.title)).toEqual(["Deleted demo.zip"]);
  });

  it("claims focus only while open, then flushes deferred toasts on close", () => {
    const { rerender } = render(<Harness open={false} />);

    rerender(<Harness open />);
    expect(isModalFocusActive()).toBe(true);

    act(() => {
      useToastStore.getState().addToast({ tone: "info", title: "Link copied" });
    });
    expect(useToastStore.getState().toasts).toHaveLength(0);

    act(() => {
      rerender(<Harness open={false} />);
    });
    expect(isModalFocusActive()).toBe(false);
    expect(useToastStore.getState().toasts.map((toast) => toast.title)).toEqual(["Link copied"]);
  });

  // Regression: dialogs opened from shortcuts or the palette have no
  // DialogTrigger, and Radix sent focus to <body> on close, so arrow keys
  // stopped reaching the task list until the user clicked a row.
  it("returns focus to the element that was focused before a trigger-less dialog opened", async () => {
    function FocusHarness({ open }: { open: boolean }) {
      return (
        <>
          <button type="button">task row</button>
          <Harness open={open} />
        </>
      );
    }
    const { getByText, rerender } = render(<FocusHarness open={false} />);
    const row = getByText("task row");
    row.focus();

    rerender(<FocusHarness open />);
    await waitFor(() => expect(document.activeElement).not.toBe(row));

    act(() => {
      rerender(<FocusHarness open={false} />);
    });
    await waitFor(() => expect(document.activeElement).toBe(row));
  });
});
