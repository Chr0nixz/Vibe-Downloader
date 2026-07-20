import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { OnboardingDialog, shouldShowOnboarding } from "./OnboardingDialog";

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, values?: Record<string, string | number>) =>
        values ? `${key} ${Object.values(values).join(" ")}` : key,
    }),
  };
});

vi.mock("@/lib/tauri", () => ({
  getBrowserIntegrationStatus: vi.fn(async () => ({
    browsers: [],
    nativeHostReady: false,
    captureAvailable: false,
  })),
}));

describe("OnboardingDialog (UX-15)", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    localStorage.clear();
  });

  it("does not permanently complete onboarding when dismissed via overlay close", () => {
    const onOpenChange = vi.fn();
    render(<OnboardingDialog open onOpenChange={onOpenChange} />);

    // Radix Dialog overlay/Escape route through onOpenChange(false).
    fireEvent.keyDown(document, { key: "Escape" });

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(shouldShowOnboarding()).toBe(true);
  });

  it("permanently completes onboarding when Skip is clicked", () => {
    const onOpenChange = vi.fn();
    render(<OnboardingDialog open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByRole("button", { name: "onboarding.skip" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(shouldShowOnboarding()).toBe(false);
  });

  it("permanently completes onboarding when Get started finishes the last step", () => {
    const onOpenChange = vi.fn();
    render(<OnboardingDialog open onOpenChange={onOpenChange} />);

    fireEvent.click(screen.getByRole("button", { name: "onboarding.next" }));
    fireEvent.click(screen.getByRole("button", { name: "onboarding.next" }));
    fireEvent.click(screen.getByRole("button", { name: "onboarding.start" }));

    expect(shouldShowOnboarding()).toBe(false);
  });
});
