import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { CloseRequestPayload } from "@/generated/bindings";
import { CloseDownloadDialog } from "./CloseDownloadDialog";

vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const request: CloseRequestPayload = {
  active: 2,
  queued: 1,
  statsUnavailable: false,
};

describe("CloseDownloadDialog", () => {
  it("submits the selected close action and remember flag", async () => {
    const onAction = vi.fn().mockResolvedValue(undefined);
    render(<CloseDownloadDialog request={request} open onCancel={vi.fn()} onAction={onAction} />);

    fireEvent.click(screen.getByRole("checkbox", { name: "closeDialog.remember" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "closeDialog.pauseExit" }));
    });

    expect(onAction).toHaveBeenCalledWith("pause_exit", true);
  });

  it("keeps the decision dialog explicit when task stats are unavailable", () => {
    render(
      <CloseDownloadDialog
        request={{ ...request, statsUnavailable: true }}
        open
        onCancel={vi.fn()}
        onAction={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(screen.getByText("closeDialog.statsUnavailable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "closeDialog.tray" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "closeDialog.pauseExit" })).toBeInTheDocument();
  });

  it("disables every close action while an action is resolving", async () => {
    let resolve!: () => void;
    const onAction = vi.fn().mockReturnValue(
      new Promise<void>((done) => {
        resolve = done;
      }),
    );
    render(<CloseDownloadDialog request={request} open onCancel={vi.fn()} onAction={onAction} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "closeDialog.tray" }));
    });
    expect(screen.getByRole("button", { name: "closeDialog.cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "closeDialog.pauseExit" })).toBeDisabled();
    resolve();
    await waitFor(() => expect(screen.getByRole("button", { name: "closeDialog.cancel" })).not.toBeDisabled());
  });
});
