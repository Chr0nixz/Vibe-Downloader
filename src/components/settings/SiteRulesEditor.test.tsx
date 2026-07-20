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
      t: (key: string) => key,
      i18n: { language: "en" },
    }),
  };
});

describe("SiteRulesEditor UX-06", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
  });

  it("does not persist on Add or Cancel", () => {
    const onUpdate = vi.fn();
    render(<SiteRulesEditor rules={[]} onUpdate={onUpdate} />);

    fireEvent.click(screen.getByRole("button", { name: "settings.addSiteRule" }));
    expect(onUpdate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "settings.cancelRule" }));
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.queryByLabelText("settings.ruleHostPattern")).not.toBeInTheDocument();
  });

  it("rejects empty host on Save", () => {
    const onUpdate = vi.fn();
    render(<SiteRulesEditor rules={[]} onUpdate={onUpdate} />);
    fireEvent.click(screen.getByRole("button", { name: "settings.addSiteRule" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.saveRule" }));
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("settings.siteRuleHostRequired");
  });

  it("saves a valid new rule and supports undo delete", () => {
    const onUpdate = vi.fn();
    const existing: BrowserSiteRule = {
      id: "rule-1",
      hostPattern: "example.com",
      includeSubdomains: true,
      mode: "auto",
      minSizeBytes: null,
      fileExtensions: ["mp4"],
      forwardHeaders: null,
    };
    const { rerender } = render(<SiteRulesEditor rules={[existing]} onUpdate={onUpdate} />);

    fireEvent.click(screen.getByRole("button", { name: "settings.deleteRule" }));
    expect(onUpdate).toHaveBeenCalledWith([]);

    const toast = useToastStore.getState().toasts[0];
    expect(toast?.title).toBe("settings.siteRuleDeleted");
    toast?.action?.onClick();
    expect(onUpdate).toHaveBeenLastCalledWith([existing]);

    rerender(<SiteRulesEditor rules={[existing]} onUpdate={onUpdate} />);
  });
});
