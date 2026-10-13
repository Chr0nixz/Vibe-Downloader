import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { saveWorkflowEntry } from "@/lib/workflow-library";
import { useTaskUIStore } from "@/stores/task-ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import { SavedViewsPicker } from "./SavedViewsPicker";

vi.mock("@/lib/runtime", () => ({ isTauriRuntime: () => false }));
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
beforeEach(() => {
  localStorage.clear();
  useWorkflowStore.setState({ entries: [], loading: false, error: false, unsupportedCount: 0 });
  useTaskUIStore.setState({
    nav: "all",
    search: "",
    selectedId: "stale",
    selectedIds: ["stale"],
    selectionAnchorId: "stale",
  });
});
it("applies a persisted dynamic query atomically and clears stale selections", async () => {
  const id = crypto.randomUUID();
  await saveWorkflowEntry({
    id,
    name: "Media failures",
    data: {
      kind: "saved_view",
      value: {
        nav: "issues",
        search: "video",
        fileType: "video",
        source: "example.com",
        failure: "hls",
        resume: "resumable",
        sortKey: "created_at",
        sortDirection: "asc",
      },
    },
  });
  render(<SavedViewsPicker />);
  const details = screen.getByText("workflow.savedView").closest("details") as HTMLDetailsElement;
  await act(async () => {
    details.open = true;
    fireEvent(details, new Event("toggle"));
  });
  fireEvent.change(within(details).getByRole("combobox"), { target: { value: id } });
  fireEvent.click(within(details).getByRole("button", { name: "workflow.apply" }));
  const state = useTaskUIStore.getState();
  expect(state).toMatchObject({
    nav: "issues",
    search: "video",
    sortKey: "created_at",
    sortDirection: "asc",
    selectedId: null,
    selectedIds: [],
    selectionAnchorId: null,
    filters: { fileType: "video", source: "example.com", failure: "hls", resume: "resumable" },
  });
  const persisted = await (await import("@/lib/workflow-library")).listWorkflowLibrary();
  expect(JSON.stringify(persisted.entries[0]?.data)).not.toContain("stale");
  expect(JSON.stringify(persisted.entries[0]?.data)).not.toContain("cursor");
});
