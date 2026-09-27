import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useTaskUIStore } from "@/stores/task-store";
import { CommandBar } from "./CommandBar";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({ t: (key: string) => key }),
  };
});

function renderBar() {
  return render(
    <TooltipProvider>
      <CommandBar platform="windows" onOpenPalette={vi.fn()} onNewDownload={vi.fn()} />
    </TooltipProvider>,
  );
}

const searchBox = () => screen.getByRole("textbox", { name: "commandBar.searchAria" }) as HTMLInputElement;

function typeAndCommit(value: string) {
  fireEvent.change(searchBox(), { target: { value } });
  act(() => {
    vi.advanceTimersByTime(400);
  });
}

describe("CommandBar search", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useTaskUIStore.setState({ search: "", nav: "all" });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // Regression: the sync effect re-ran when the store changed, saw the stale
  // debounced text differ from the cleared store, and wrote it straight back.
  it("stays cleared after the clear button, past the debounce window", () => {
    renderBar();
    typeAndCommit("zzqx-nothing");
    expect(useTaskUIStore.getState().search).toBe("zzqx-nothing");

    fireEvent.click(screen.getByRole("button", { name: "settings.clearSearch" }));
    act(() => {
      vi.advanceTimersByTime(400);
    });

    expect(useTaskUIStore.getState().search).toBe("");
    expect(searchBox().value).toBe("");
  });

  it("adopts a clear made elsewhere, such as the empty state's Clear search", () => {
    renderBar();
    typeAndCommit("ubuntu");

    act(() => {
      useTaskUIStore.getState().setSearch("");
    });
    act(() => {
      vi.advanceTimersByTime(400);
    });

    expect(useTaskUIStore.getState().search).toBe("");
    expect(searchBox().value).toBe("");
  });

  it("clears a query with Escape", () => {
    renderBar();
    typeAndCommit("iso");

    fireEvent.keyDown(searchBox(), { key: "Escape" });
    act(() => {
      vi.advanceTimersByTime(400);
    });

    expect(useTaskUIStore.getState().search).toBe("");
  });
});
