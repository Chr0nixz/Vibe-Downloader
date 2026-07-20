import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { BrowserCaptureControls } from "@/components/settings/BrowserCaptureControls";
import type { BrowserCaptureSettings } from "@/generated/bindings";

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

const BASE: BrowserCaptureSettings = {
  experimentalCaptureEnabled: true,
  autoIntercept: false,
  forwardHeaders: false,
  forwardHeadersMode: "ask",
  minSizeBytes: "0",
  fileExtensions: ["mp4"],
  siteRules: [],
  allowIntranetHandoff: false,
};

describe("BrowserCaptureControls UX-07", () => {
  it("keeps a single three-state forward-headers control and preserves ask", () => {
    const onUpdate = vi.fn();
    render(<BrowserCaptureControls settings={BASE} available onUpdate={onUpdate} />);

    expect(screen.getByLabelText("settings.browserForwardHeadersMode")).toBeInTheDocument();
    expect(screen.queryByLabelText("settings.browserForwardHeaders")).not.toBeInTheDocument();
    // Select shows ask as the current value; no binary switch can rewrite it.
    expect(screen.getByRole("combobox", { name: "settings.browserForwardHeadersMode" })).toHaveTextContent(
      "settings.browserForwardHeadersAsk",
    );
  });
});
