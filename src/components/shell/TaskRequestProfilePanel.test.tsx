import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskRequestProfileView } from "@/generated/bindings";
import type { Task } from "@/types/task";
import { TaskRequestProfilePanel } from "./TaskRequestProfilePanel";

const mocks = vi.hoisted(() => ({ get: vi.fn(), update: vi.fn() }));
vi.mock("@/lib/tauri", () => ({ getTaskRequestProfile: mocks.get, updateTaskRequestProfile: mocks.update }));
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
const view: TaskRequestProfileView = {
  taskId: "task",
  userAgent: "Agent/1",
  referer: null,
  customHeaders: [{ name: "accept", value: "application/octet-stream" }],
  sensitiveHeaderNames: ["cookie", "x-token"],
  sensitiveExpiresAt: "2026-10-03T00:00:00Z",
  sensitiveExpired: false,
};
const task = { id: "task", status: "paused" } as Task;

beforeEach(() => {
  vi.clearAllMocks();
  mocks.get.mockResolvedValue(view);
  mocks.update.mockResolvedValue(view);
});

describe("task request profile editing", () => {
  it("preserves stored sensitive headers during an ordinary edit", async () => {
    render(<TaskRequestProfilePanel task={task} />);
    await waitFor(() => expect(screen.getByLabelText("User-Agent")).toHaveValue("Agent/1"));
    expect(screen.getByLabelText("requestProfile.customHeaders")).toHaveValue("accept: application/octet-stream");
    fireEvent.change(screen.getByLabelText("User-Agent"), { target: { value: "Agent/2" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "requestProfile.save" })));
    expect(mocks.update).toHaveBeenCalledWith(
      "task",
      { userAgent: "Agent/2", referer: null, customHeaders: [{ name: "accept", value: "application/octet-stream" }] },
      false,
    );
  });
  it("requires an explicit replacement and clears submitted secret values after save", async () => {
    render(<TaskRequestProfilePanel task={task} />);
    await waitFor(() => expect(screen.getByLabelText("User-Agent")).toHaveValue("Agent/1"));
    fireEvent.change(screen.getByLabelText("requestProfile.customHeaders"), {
      target: { value: "Cookie: session=fixture" },
    });
    fireEvent.click(screen.getByLabelText("requestProfile.replaceSensitive", { exact: false }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "requestProfile.save" })));
    expect(mocks.update).toHaveBeenCalledWith(
      "task",
      expect.objectContaining({ customHeaders: [{ name: "cookie", value: "session=fixture" }] }),
      true,
    );
    expect(screen.getByLabelText("requestProfile.customHeaders")).not.toHaveValue(
      expect.stringContaining("session=fixture"),
    );
  });
  it("marks expired secrets for replacement and disables editing on active tasks", async () => {
    mocks.get.mockResolvedValue({ ...view, sensitiveExpired: true });
    const rendered = render(<TaskRequestProfilePanel task={task} />);
    await screen.findByText("requestProfile.expired");
    expect(screen.getByLabelText("requestProfile.replaceSensitive", { exact: false })).toBeChecked();
    rendered.rerender(<TaskRequestProfilePanel task={{ ...task, status: "downloading" }} />);
    expect(screen.getByRole("button", { name: "requestProfile.save" })).toBeDisabled();
    expect(screen.getByLabelText("User-Agent")).toBeDisabled();
  });
  it("discards a save response after the user switches tasks", async () => {
    let resolve!: (result: TaskRequestProfileView) => void;
    mocks.update.mockReturnValue(
      new Promise<TaskRequestProfileView>((done) => {
        resolve = done;
      }),
    );
    const rendered = render(<TaskRequestProfilePanel task={task} />);
    await waitFor(() => expect(screen.getByLabelText("User-Agent")).toHaveValue("Agent/1"));
    fireEvent.click(screen.getByRole("button", { name: "requestProfile.save" }));
    mocks.get.mockResolvedValue({ ...view, taskId: "next", userAgent: "Next/1" });
    rendered.rerender(<TaskRequestProfilePanel task={{ ...task, id: "next" }} />);
    await waitFor(() => expect(screen.getByLabelText("User-Agent")).toHaveValue("Next/1"));
    await act(async () => resolve({ ...view, userAgent: "Old/2" }));
    expect(screen.getByLabelText("User-Agent")).toHaveValue("Next/1");
  });
});
