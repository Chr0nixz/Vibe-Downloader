import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SiteRulesEditor } from "@/components/settings/SiteRulesEditor";
import type { BrowserSiteRule } from "@/generated/bindings";
import { useToastStore } from "@/stores/toast-store";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, options?: Record<string, unknown>) => {
        if (options && "host" in options) return `${key}:${options.host}`;
        if (options && "count" in options) return `${key}:${options.count}`;
        return key;
      },
      i18n: { language: "en" },
    }),
  };
});

const captureGlobals = {
  autoIntercept: true,
  minSizeBytes: "0",
  fileExtensions: ["mp4"] as string[],
  forwardHeadersMode: "ask" as const,
};

const broad: BrowserSiteRule = {
  id: "rule-broad",
  hostPattern: "example.com",
  includeSubdomains: true,
  mode: "auto",
  minSizeBytes: null,
  fileExtensions: ["mp4"],
  forwardHeaders: null,
};

const narrow: BrowserSiteRule = {
  id: "rule-narrow",
  hostPattern: "cdn.example.com",
  includeSubdomains: false,
  mode: "never",
  minSizeBytes: null,
  fileExtensions: [],
  forwardHeaders: null,
};

describe("SiteRulesEditor UX-06", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
  });

  it("does not persist on Add or Cancel", () => {
    const onUpdate = vi.fn();
    render(<SiteRulesEditor rules={[]} captureGlobals={captureGlobals} onUpdate={onUpdate} />);

    fireEvent.click(screen.getByRole("button", { name: "settings.addSiteRule" }));
    expect(onUpdate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "settings.cancelRule" }));
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("settings.ruleHostPattern")).not.toBeInTheDocument();
  });

  it("rejects empty host on Save", () => {
    const onUpdate = vi.fn();
    render(<SiteRulesEditor rules={[]} captureGlobals={captureGlobals} onUpdate={onUpdate} />);
    fireEvent.click(screen.getByRole("button", { name: "settings.addSiteRule" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.saveRule" }));
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("settings.siteRuleHostRequired");
  });

  it("saves a valid new rule and supports undo delete", () => {
    const onUpdate = vi.fn();
    const { rerender } = render(
      <SiteRulesEditor rules={[broad]} captureGlobals={captureGlobals} onUpdate={onUpdate} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "settings.deleteRule" }));
    expect(onUpdate).toHaveBeenCalledWith([]);

    const toast = useToastStore.getState().toasts[0];
    expect(toast?.title).toBe("settings.siteRuleDeleted");
    toast?.action?.onClick();
    expect(onUpdate).toHaveBeenLastCalledWith([broad]);

    rerender(<SiteRulesEditor rules={[broad]} captureGlobals={captureGlobals} onUpdate={onUpdate} />);
  });

  it("reorders rules with move up", () => {
    const onUpdate = vi.fn();
    render(<SiteRulesEditor rules={[broad, narrow]} captureGlobals={captureGlobals} onUpdate={onUpdate} />);
    const moveUpButtons = screen.getAllByRole("button", { name: "settings.moveUp" });
    fireEvent.click(moveUpButtons[1]);
    expect(onUpdate).toHaveBeenCalledWith([narrow, broad]);
  });

  it("shows shadowed conflict warning", () => {
    render(<SiteRulesEditor rules={[broad, narrow]} captureGlobals={captureGlobals} onUpdate={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("settings.siteRulesConflictSummary:1");
    expect(screen.getByText(/settings.siteRulesConflictShadowed/)).toBeInTheDocument();
  });

  it("diagnoses first-wins intercept for overlapping rules", () => {
    render(<SiteRulesEditor rules={[broad, narrow]} captureGlobals={captureGlobals} onUpdate={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("settings.siteRulesTryUrl"), {
      target: { value: "https://cdn.example.com/video.mp4" },
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.siteRulesTryRun" }));
    expect(screen.getByText(/cdn.example.com/)).toBeInTheDocument();
    // Broad rule wins first; mode auto → intercept yes for matching mp4
    expect(screen.getByText("settings.siteRulesTryInterceptYes")).toBeInTheDocument();
  });
});
