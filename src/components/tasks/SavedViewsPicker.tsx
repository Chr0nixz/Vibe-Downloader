import { WorkflowPicker } from "@/components/shell/WorkflowPicker";
import { bumpListQueryEpoch } from "@/lib/list-query-epoch";
import type { FileTypeFilter, NavFilter, ResumeFilter, TaskSortDirection, TaskSortKey } from "@/stores/task-data-store";
import { effectiveListQueryMembership } from "@/stores/task-query";
import { useTaskUIStore } from "@/stores/task-ui-store";

export function SavedViewsPicker() {
  return (
    <WorkflowPicker
      kind="saved_view"
      getData={() => {
        const state = useTaskUIStore.getState();
        const query = effectiveListQueryMembership(state.nav, state.search, state.filters);
        return {
          kind: "saved_view",
          value: {
            nav: query.nav,
            search: query.search,
            sortKey: state.sortKey,
            sortDirection: state.sortDirection,
            fileType: query.filters.fileType,
            source: query.filters.source,
            failure: query.filters.failure,
            resume: query.filters.resume,
          },
        };
      }}
      onApply={(data) => {
        if (data.kind !== "saved_view") return;
        const view = data.value;
        bumpListQueryEpoch();
        useTaskUIStore.setState({
          nav: view.nav as NavFilter,
          search: view.search,
          sortKey: view.sortKey as TaskSortKey,
          sortDirection: view.sortDirection as TaskSortDirection,
          filters: {
            fileType: view.fileType as FileTypeFilter,
            source: view.source,
            failure: view.failure,
            resume: view.resume as ResumeFilter,
          },
          selectedId: null,
          selectedIds: [],
          selectionAnchorId: null,
        });
      }}
    />
  );
}
