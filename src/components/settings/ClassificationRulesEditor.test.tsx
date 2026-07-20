import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ClassificationRulesEditor } from "@/components/settings/ClassificationRulesEditor";
import type { ClassificationRule, PreviewClassificationResult } from "@/generated/bindings";
import { useToastStore } from "@/stores/toast-store";

const listClassificationRules = vi.fn();
const previewClassificationMatch = vi.fn();

vi.mock("@/lib/tauri", () => ({
  listClassificationRules: () => listClassificationRules(),
  createClassificationRule: vi.fn(),
  updateClassificationRule: vi.fn(),
  deleteClassificationRule: vi.fn(),
  reorderClassificationRules: vi.fn(),
  previewClassificationMatch: (input: unknown) => previewClassificationMatch(input),
}));

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

const sampleRule: ClassificationRule = {
  id: "rule-1",
  name: "Videos",
  enabled: true,
  position: 0,
  matchKind: "extension",
  pattern: "mp4",
  targetSubdir: "videos",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const previewHit: PreviewClassificationResult = {
  matched: true,
  manualOverride: false,
  targetSubdir: "videos",
  matchedRule: sampleRule,
  effectiveSaveDir: "C:\\Downloads\\videos",
  inputsUsed: {
    url: "https://example.com/video/file.mp4",
    fileName: "file.mp4",
    contentType: "video/mp4",
  },
};

describe("ClassificationRulesEditor try panel", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    listClassificationRules.mockReset();
    previewClassificationMatch.mockReset();
    listClassificationRules.mockResolvedValue([sampleRule]);
    previewClassificationMatch.mockResolvedValue(previewHit);
  });

  it("previews a matched classification rule", async () => {
    render(<ClassificationRulesEditor />);
    await waitFor(() => expect(listClassificationRules).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "settings.classificationTryRun" }));
    await waitFor(() => expect(previewClassificationMatch).toHaveBeenCalled());
    expect(previewClassificationMatch).toHaveBeenCalledWith({
      url: "https://example.com/video/file.mp4",
      fileName: "file.mp4",
      contentType: "video/mp4",
      categoryKey: null,
    });
    expect(await screen.findByText("C:\\Downloads\\videos")).toBeInTheDocument();
  });

  it("shows manual override messaging", async () => {
    previewClassificationMatch.mockResolvedValue({
      ...previewHit,
      matched: true,
      manualOverride: true,
      matchedRule: null,
      targetSubdir: "manual",
      effectiveSaveDir: "C:\\Downloads\\manual",
    });
    render(<ClassificationRulesEditor />);
    await waitFor(() => expect(listClassificationRules).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("settings.classificationTryCategory"), {
      target: { value: "manual" },
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.classificationTryRun" }));
    expect(await screen.findByText("settings.classificationTryManualOverride")).toBeInTheDocument();
  });
});
