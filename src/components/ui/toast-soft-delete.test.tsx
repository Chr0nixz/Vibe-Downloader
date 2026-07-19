import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ToastViewport } from "@/components/ui/toast";
import { UNDO_TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, values?: Record<string, string | number>) =>
        values ? `${key} ${Object.values(values).join(" ")}` : key,
      i18n: { language: "en" },
    }),
  };
});

vi.mock("motion/react", async () => {
  const React = await import("react");
  return {
    AnimatePresence: ({ children }: { children: React.ReactNode }) =>
      React.createElement(React.Fragment, null, children),
    motion: {
      div: (props: React.HTMLAttributes<HTMLDivElement> & { children?: React.ReactNode }) =>
        React.createElement("div", props),
    },
    useReducedMotion: () => true,
  };
});

describe("toast soft-delete lifecycle", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useToastStore.setState({ toasts: [] });
  });

  afterEach(() => {
    useToastStore.setState({ toasts: [] });
    vi.useRealTimers();
  });

  it("commits on timeout and not before", () => {
    const onAutoCommit = vi.fn();
    const onUndo = vi.fn();

    render(<ToastViewport />);
    act(() => {
      useToastStore.getState().addToast({
        tone: "info",
        title: "Deleted demo.zip",
        description: "Undo hint",
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        onAutoCommit,
        action: { label: "toast.undo", onClick: onUndo },
      });
    });

    act(() => {
      vi.advanceTimersByTime(UNDO_TOAST_TIMEOUT_MS - 1);
    });
    expect(onAutoCommit).not.toHaveBeenCalled();
    expect(onUndo).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onAutoCommit).toHaveBeenCalledTimes(1);
    expect(onUndo).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("hover pause delays commit so Undo stays available past the default window", () => {
    const onAutoCommit = vi.fn();
    const onUndo = vi.fn();

    render(<ToastViewport />);
    act(() => {
      useToastStore.getState().addToast({
        tone: "info",
        title: "Deleted demo.zip",
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        onAutoCommit,
        action: { label: "toast.undo", onClick: onUndo },
      });
    });

    const toast = screen.getByRole("status");
    fireEvent.mouseEnter(toast);

    act(() => {
      vi.advanceTimersByTime(UNDO_TOAST_TIMEOUT_MS + 2_000);
    });
    expect(onAutoCommit).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "toast.undo" }));
    expect(onUndo).toHaveBeenCalledTimes(1);
    expect(onAutoCommit).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("dismiss X commits without calling Undo", () => {
    const onAutoCommit = vi.fn();
    const onUndo = vi.fn();

    render(<ToastViewport />);
    act(() => {
      useToastStore.getState().addToast({
        tone: "info",
        title: "Deleted demo.zip",
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        onAutoCommit,
        action: { label: "toast.undo", onClick: onUndo },
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "toast.dismiss" }));
    expect(onAutoCommit).toHaveBeenCalledTimes(1);
    expect(onUndo).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("clearToasts commits pending soft-deletes", () => {
    const onAutoCommit = vi.fn();
    const onUndo = vi.fn();

    act(() => {
      useToastStore.getState().addToast({
        tone: "info",
        title: "Deleted demo.zip",
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        onAutoCommit,
        action: { label: "toast.undo", onClick: onUndo },
      });
    });

    act(() => {
      useToastStore.getState().clearToasts();
    });
    expect(onAutoCommit).toHaveBeenCalledTimes(1);
    expect(onUndo).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("Undo settles once and blocks a later commit", () => {
    let settled = false;
    const onAutoCommit = vi.fn(() => {
      if (settled) return;
      settled = true;
    });
    const onUndo = vi.fn(() => {
      if (settled) return;
      settled = true;
    });

    render(<ToastViewport />);
    act(() => {
      useToastStore.getState().addToast({
        tone: "info",
        title: "Deleted demo.zip",
        durationMs: UNDO_TOAST_TIMEOUT_MS,
        onAutoCommit,
        action: { label: "toast.undo", onClick: onUndo },
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "toast.undo" }));
    expect(onUndo).toHaveBeenCalledTimes(1);
    expect(onAutoCommit).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(UNDO_TOAST_TIMEOUT_MS + 1_000);
    });
    expect(onAutoCommit).not.toHaveBeenCalled();
  });
});
