/**
 * The row that inherits the selection when `removed` leave a list ordered as
 * `order`: the first surviving row after the selected one, else the nearest
 * surviving row before it, the way a desktop file list moves on after Delete.
 * Rows in `hidden` (already inside a soft-delete undo window) never survive.
 * Returns null when the selected row is not in `order` or nothing survives.
 */
export function selectionSuccessor(
  order: readonly string[],
  selectedId: string,
  removed: ReadonlySet<string>,
  hidden: ReadonlySet<string> = new Set(),
): string | null {
  const index = order.indexOf(selectedId);
  if (index === -1) return null;
  const survives = (id: string) => !removed.has(id) && !hidden.has(id);
  for (let i = index + 1; i < order.length; i++) {
    const id = order[i]!;
    if (survives(id)) return id;
  }
  for (let i = index - 1; i >= 0; i--) {
    const id = order[i]!;
    if (survives(id)) return id;
  }
  return null;
}
