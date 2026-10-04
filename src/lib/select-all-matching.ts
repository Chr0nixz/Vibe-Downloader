import type { ListTasksCursorInput } from "@/generated/bindings";
import type { TaskCursorPage } from "@/lib/tauri";

/** The query fields that must remain unchanged while cursor pages are loaded. */
export function cursorQueryKey(input: ListTasksCursorInput): string {
  const { cursor: _cursor, ...query } = input;
  return JSON.stringify(query);
}

export class SelectAllPaginationError extends Error {
  constructor() {
    super("task selection pagination did not advance");
    this.name = "SelectAllPaginationError";
  }
}

export type SelectAllMatchingOptions = {
  initialIds: readonly string[];
  pendingIds?: readonly string[];
  nextCursor: string | null;
  loadPage: (cursor: string) => Promise<TaskCursorPage>;
  appendPage: (page: TaskCursorPage) => void;
  isCurrent: () => boolean;
};

export type SelectAllMatchingResult = {
  ids: string[];
  completed: boolean;
};

/**
 * Load every remaining page for one fixed list query and collect its IDs.
 *
 * The caller owns query invalidation through `isCurrent`; a stale response is
 * never appended or selected. Repeated cursors are treated as an error so a
 * malformed adapter cannot spin forever.
 */
export async function collectAllMatchingTaskIds({
  initialIds,
  pendingIds = [],
  nextCursor,
  loadPage,
  appendPage,
  isCurrent,
}: SelectAllMatchingOptions): Promise<SelectAllMatchingResult> {
  const pending = new Set(pendingIds);
  const ids = new Set(initialIds.filter((id) => !pending.has(id)));
  const seenCursors = new Set<string>();
  let cursor = nextCursor;

  while (cursor) {
    if (!isCurrent()) return { ids: [...ids], completed: false };
    if (seenCursors.has(cursor)) throw new SelectAllPaginationError();
    seenCursors.add(cursor);

    const page = await loadPage(cursor);
    if (!isCurrent()) return { ids: [...ids], completed: false };

    appendPage(page);
    for (const task of page.items) {
      if (!pending.has(task.id)) ids.add(task.id);
    }

    if (page.nextCursor && seenCursors.has(page.nextCursor)) {
      throw new SelectAllPaginationError();
    }
    cursor = page.nextCursor;
  }

  return { ids: [...ids], completed: true };
}
