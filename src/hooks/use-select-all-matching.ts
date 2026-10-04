import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { bumpListQueryEpoch, isCurrentListQueryEpoch } from "@/lib/list-query-epoch";
import { collectAllMatchingTaskIds, cursorQueryKey } from "@/lib/select-all-matching";
import { listTasksCursor } from "@/lib/tauri";
import { taskCursorInput, useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";

/**
 * Shared implementation for Ctrl/Command+A, the list context menu, and the
 * command palette. Cursor pages are loaded into the entity cache before the
 * final selection is published, so bulk actions can see every selected task.
 */
export function useSelectAllMatching(): {
  selectAllMatching: () => Promise<void>;
  selectingAll: boolean;
} {
  const { t } = useTranslation();
  const addToast = useToastStore((state) => state.addToast);
  const setSelectedIds = useTaskUIStore((state) => state.setSelectedIds);
  const setTaskCursorPage = useTaskDataStore((state) => state.setTaskCursorPage);
  const [selectingAll, setSelectingAll] = useState(false);
  const requestRef = useRef(0);

  useEffect(
    () => () => {
      requestRef.current += 1;
    },
    [],
  );

  const selectAllMatching = useCallback(async () => {
    const requestId = ++requestRef.current;
    setSelectingAll(false);
    const data = useTaskDataStore.getState();
    // A replace load means the store still describes the previous query. Let
    // that load settle instead of starting from its cursor under a new query.
    if (data.loading) return;

    const query = taskCursorInput(null);
    const queryKey = cursorQueryKey(query);
    // Invalidate an in-flight infinite-scroll append. This operation owns the
    // cursor chain until it finishes; stale append responses are ignored by
    // the shared epoch check in TaskList/AppShell.
    let epoch = bumpListQueryEpoch();
    const pendingIds = useTaskUIStore.getState().pendingDeleteIds;
    const initialIds = data.taskIds;

    const isCurrent = () =>
      requestRef.current === requestId &&
      isCurrentListQueryEpoch(epoch) &&
      cursorQueryKey(taskCursorInput(null)) === queryKey;

    if (!data.nextCursor) {
      if (isCurrent()) {
        setSelectedIds(initialIds.filter((id) => !pendingIds.includes(id)));
      }
      return;
    }

    setSelectingAll(true);
    try {
      const result = await collectAllMatchingTaskIds({
        initialIds,
        pendingIds,
        nextCursor: data.nextCursor,
        loadPage: (cursor) => listTasksCursor({ ...query, cursor }),
        appendPage: (page) => {
          setTaskCursorPage(page.items, page.minimumTotal, page.nextCursor, page.filterOptions, true);
          // A virtualized list can start its own append when the newly added
          // page reaches the viewport threshold. Advance the shared epoch for
          // every page so a response from that competing append cannot write
          // an older cursor after this selection chain has moved on.
          epoch = bumpListQueryEpoch();
        },
        isCurrent,
      });
      if (result.completed && isCurrent()) setSelectedIds(result.ids);
    } catch {
      if (isCurrent()) {
        addToast({ tone: "error", title: t("toast.selectAllFailed") });
      }
    } finally {
      if (requestRef.current === requestId) setSelectingAll(false);
    }
  }, [addToast, setSelectedIds, setTaskCursorPage, t]);

  return { selectAllMatching, selectingAll };
}
