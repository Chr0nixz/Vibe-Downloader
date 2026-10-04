import { describe, expect, it, vi } from "vitest";

import { collectAllMatchingTaskIds, cursorQueryKey, SelectAllPaginationError } from "./select-all-matching";

function page(ids: string[], nextCursor: string | null) {
  return {
    items: ids.map((id) => ({ id }) as never),
    nextCursor,
    minimumTotal: ids.length,
    filterOptions: { sources: [], failureCategories: [] },
  };
}

describe("select all matching pagination", () => {
  it("collects every page, appends entities, and excludes pending deletions", async () => {
    const appendPage = vi.fn();
    const loadPage = vi.fn(async (cursor: string) =>
      cursor === "page-1" ? page(["b", "deleted"], "page-2") : page(["c"], null),
    );

    const result = await collectAllMatchingTaskIds({
      initialIds: ["a"],
      pendingIds: ["deleted"],
      nextCursor: "page-1",
      loadPage,
      appendPage,
      isCurrent: () => true,
    });

    expect(result).toEqual({ ids: ["a", "b", "c"], completed: true });
    expect(loadPage).toHaveBeenNthCalledWith(1, "page-1");
    expect(loadPage).toHaveBeenNthCalledWith(2, "page-2");
    expect(appendPage).toHaveBeenCalledTimes(2);
  });

  it("does not append or select a page after the query becomes stale", async () => {
    let current = true;
    const appendPage = vi.fn();

    const result = await collectAllMatchingTaskIds({
      initialIds: ["a"],
      nextCursor: "page-1",
      loadPage: async () => {
        current = false;
        return page(["stale"], null);
      },
      appendPage,
      isCurrent: () => current,
    });

    expect(result).toEqual({ ids: ["a"], completed: false });
    expect(appendPage).not.toHaveBeenCalled();
  });

  it("fails closed when an adapter repeats a cursor", async () => {
    await expect(
      collectAllMatchingTaskIds({
        initialIds: [],
        nextCursor: "same",
        loadPage: async () => page(["a"], "same"),
        appendPage: () => {},
        isCurrent: () => true,
      }),
    ).rejects.toBeInstanceOf(SelectAllPaginationError);
  });

  it("keys the query without the mutable cursor", () => {
    const first = cursorQueryKey({
      nav: "all",
      search: "zip",
      sortKey: "updated_at",
      sortDirection: "desc",
      fileType: "all",
      source: "all",
      failureCategory: "all",
      resume: "all",
      cursor: "one",
      pageSize: 100,
    });
    const second = cursorQueryKey({
      nav: "all",
      search: "zip",
      sortKey: "updated_at",
      sortDirection: "desc",
      fileType: "all",
      source: "all",
      failureCategory: "all",
      resume: "all",
      cursor: "two",
      pageSize: 100,
    });

    expect(first).toBe(second);
  });
});
