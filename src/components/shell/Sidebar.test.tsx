import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { Sidebar } from "./Sidebar";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({ t: (key: string) => key }),
  };
});

function renderSidebar() {
  return render(
    <TooltipProvider>
      <Sidebar />
    </TooltipProvider>,
  );
}

const desktopNav = () => within(screen.getAllByRole("navigation", { name: "app.navAria" })[0]);

describe("Sidebar task-view grouping", () => {
  beforeEach(() => {
    localStorage.removeItem("vibe-sidebar-collapsed");
    useTaskUIStore.setState({ nav: "all" });
    useTaskDataStore.setState({ globalTaskStats: null });
  });

  it("keeps four core destinations visible and groups lower-frequency task views", () => {
    renderSidebar();

    for (const name of ["nav.all", "nav.downloading", "nav.issues", "nav.completed"]) {
      expect(desktopNav().getByRole("button", { name })).toBeInTheDocument();
    }
    // Failed and Needs attention are one destination now, not two entries.
    expect(desktopNav().queryByRole("button", { name: "nav.failed" })).not.toBeInTheDocument();
    expect(desktopNav().queryByRole("button", { name: "attentionCenter.title" })).not.toBeInTheDocument();
    expect(desktopNav().queryByRole("button", { name: "nav.queue" })).not.toBeInTheDocument();
    expect(desktopNav().queryByRole("button", { name: "nav.paused" })).not.toBeInTheDocument();

    const otherViews = desktopNav().getByRole("button", { name: "nav.otherViews" });
    expect(otherViews).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(otherViews);

    expect(otherViews).toHaveAttribute("aria-expanded", "true");
    expect(desktopNav().getByRole("button", { name: "nav.queue" })).toBeInTheDocument();
    expect(desktopNav().getByRole("button", { name: "nav.paused" })).toBeInTheDocument();
    fireEvent.click(desktopNav().getByRole("button", { name: "nav.queue" }));
    expect(useTaskUIStore.getState().nav).toBe("queue");
  });

  it("opens the group when the current view is one of its task states", () => {
    useTaskUIStore.setState({ nav: "paused" });
    renderSidebar();

    expect(desktopNav().getByRole("button", { name: "nav.otherViews" })).toHaveAttribute("aria-expanded", "true");
    expect(desktopNav().getByRole("button", { name: "nav.paused" })).toHaveAttribute("aria-current", "page");
  });

  it.each(["issues", "attention", "failed"] as const)("marks Needs you as current for the %s cause view", (nav) => {
    useTaskUIStore.setState({ nav });
    renderSidebar();

    expect(desktopNav().getByRole("button", { name: "nav.issues" })).toHaveAttribute("aria-current", "page");
  });
});
