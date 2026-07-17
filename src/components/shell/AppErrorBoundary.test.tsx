import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { axe } from "jest-axe";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AppErrorBoundary } from "./AppErrorBoundary";

let activeLanguage: "en" | "zh-CN" = "en";

const translations = {
  en: {
    "errorBoundary.title": "Something went wrong",
    "errorBoundary.description": "The app hit an unexpected error.",
    "errorBoundary.reload": "Reload",
    "errorBoundary.copyError": "Copy error",
    "errorBoundary.home": "Go home",
  },
  "zh-CN": {
    "errorBoundary.title": "应用出现异常",
    "errorBoundary.description": "应用遇到了意外错误。",
    "errorBoundary.reload": "重新加载",
    "errorBoundary.copyError": "复制错误",
    "errorBoundary.home": "返回主页",
  },
} as const;

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, options?: { defaultValue?: string }) =>
        translations[activeLanguage][key as keyof (typeof translations)["en"]] ?? options?.defaultValue ?? key,
    }),
  };
});

function BrokenView({ shouldThrow }: { shouldThrow: () => boolean }) {
  if (shouldThrow()) throw new Error("render failed");
  return <p>Recovered content</p>;
}

describe("AppErrorBoundary", () => {
  const clipboardWrite = vi.fn<(text: string) => Promise<void>>();
  let consoleError: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    activeLanguage = "en";
    clipboardWrite.mockReset();
    clipboardWrite.mockResolvedValue();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: clipboardWrite },
    });
    consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    consoleError.mockRestore();
  });

  it("shows localized recovery actions and copies diagnostic details", async () => {
    activeLanguage = "zh-CN";
    render(
      <AppErrorBoundary>
        <BrokenView shouldThrow={() => true} />
      </AppErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "应用出现异常" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制错误" }));

    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledTimes(1));
    expect(clipboardWrite.mock.calls[0]?.[0]).toContain("render failed");
  });

  it("resets the boundary without reloading the application", () => {
    let broken = true;
    render(
      <AppErrorBoundary>
        <BrokenView shouldThrow={() => broken} />
      </AppErrorBoundary>,
    );

    expect(screen.getByRole("heading", { name: "Something went wrong" })).toBeInTheDocument();
    broken = false;
    fireEvent.click(screen.getByRole("button", { name: "Go home" }));

    expect(screen.getByText("Recovered content")).toBeInTheDocument();
  });

  it("has no automated accessibility violations in the recovery surface", async () => {
    render(
      <AppErrorBoundary>
        <BrokenView shouldThrow={() => true} />
      </AppErrorBoundary>,
    );

    const results = await axe(document.body);
    expect(results.violations).toEqual([]);
  });
});
