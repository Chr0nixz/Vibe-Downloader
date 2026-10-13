import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useTaskDataStore } from "@/stores/task-data-store";
import { useTaskUIStore } from "@/stores/task-ui-store";
import { BulkTaskOptionsDialog } from "./BulkTaskOptionsDialog";

const mocks = vi.hoisted(() => ({ submit: vi.fn() }));
vi.mock("@/lib/runtime", () => ({ isTauriRuntime: () => false }));
vi.mock("@/lib/tauri-browser", () => ({ bulkUpdateTaskOptions: mocks.submit }));
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
beforeEach(() => {
  mocks.submit.mockReset();
  useTaskDataStore.setState({ taskById: {}, taskIds: [] });
  useTaskUIStore.setState({ pendingDeleteIds: [] });
});
it("submits only chosen fields and retries only unresolved task IDs", async () => {
  mocks.submit
    .mockResolvedValueOnce([
      { id: "a", status: "applied", error: null },
      { id: "b", status: "failed", error: "workflow_invalid" },
    ])
    .mockResolvedValueOnce([{ id: "b", status: "applied", error: null }]);
  const done = vi.fn();
  render(<BulkTaskOptionsDialog ids={["a", "b"]} onClose={() => {}} onComplete={done} />);
  fireEvent.change(screen.getAllByRole("combobox")[0], { target: { value: "low" } });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "workflow.apply" })));
  expect(mocks.submit).toHaveBeenNthCalledWith(1, ["a", "b"], {
    priority: "low",
    taskSpeedLimitBps: null,
    clearSpeedLimit: false,
    categoryKey: null,
    clearCategory: false,
    obeySchedule: null,
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "workflow.retry" })));
  expect(mocks.submit.mock.calls[1]?.[0]).toEqual(["b"]);
  expect(done).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("button", { name: "workflow.retry" })).toBeDisabled();
});

it("excludes tasks pending deletion and explains invalid chosen values", async () => {
  useTaskUIStore.setState({ pendingDeleteIds: ["b"] });
  mocks.submit.mockResolvedValue([{ id: "a", status: "applied", error: null }]);
  render(<BulkTaskOptionsDialog ids={["a", "b", "a"]} onClose={() => {}} onComplete={() => {}} />);
  await act(async () =>
    fireEvent.change(screen.getByRole("combobox", { name: "taskDetails.taskSpeedLimit" }), {
      target: { value: "set" },
    }),
  );
  const input = screen.getByRole("textbox", { name: /workflow.bytesPerSecond/ });
  fireEvent.change(input, { target: { value: "9007199254740992" } });
  expect(input).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByText("workflow.invalidSpeed")).toBeInTheDocument();
  expect(input).toHaveAttribute("aria-describedby", "bulk-options-speed-error");
  expect(screen.getByRole("button", { name: "workflow.apply" })).toBeDisabled();
  fireEvent.change(input, { target: { value: "4096" } });
  fireEvent.change(screen.getByRole("combobox", { name: "newDownload.category" }), { target: { value: "set" } });
  fireEvent.change(screen.getByRole("textbox", { name: /newDownload.category/ }), {
    target: { value: "中".repeat(90) },
  });
  expect(screen.getByText("workflow.invalidCategory")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "workflow.apply" })).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: /newDownload.category/ }), { target: { value: "media" } });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "workflow.apply" })));
  expect(mocks.submit.mock.calls[0]?.[0]).toEqual(["a"]);
});

it("stops subsequent chunks if the dialog is unmounted during an in-flight batch", async () => {
  let finish!: (rows: { id: string; status: string; error: null }[]) => void;
  mocks.submit.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const done = vi.fn();
  const ids = Array.from({ length: 101 }, (_, i) => String(i));
  const view = render(<BulkTaskOptionsDialog ids={ids} onClose={() => {}} onComplete={done} />);
  await act(async () =>
    fireEvent.change(screen.getByRole("combobox", { name: "taskDetails.priority" }), { target: { value: "high" } }),
  );
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "workflow.apply" })));
  view.unmount();
  await act(async () => {
    finish(ids.slice(0, 100).map((id) => ({ id, status: "applied", error: null })));
  });
  expect(mocks.submit).toHaveBeenCalledTimes(1);
  expect(done).not.toHaveBeenCalled();
});
