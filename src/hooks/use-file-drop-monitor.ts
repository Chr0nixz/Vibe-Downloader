import { useEffect, useRef } from "react";

import { type FileDropDragState, onFileDrop } from "@/lib/tauri";

type Handlers = {
  onDrop: (paths: string[]) => void;
  onDragStateChange: (state: FileDropDragState) => void;
};

/**
 * UX-23: registers the file-drop listener exactly once for the component's
 * lifetime. Handlers live in refs so changing callback identity (dialog state,
 * locale) never tears the listener down — the old inline effect unlistened and
 * re-listened across an await window in which drops were silently lost.
 */
export function useFileDropMonitor({ onDrop, onDragStateChange }: Handlers): void {
  const dropRef = useRef(onDrop);
  dropRef.current = onDrop;
  const dragRef = useRef(onDragStateChange);
  dragRef.current = onDragStateChange;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      unlisten = await onFileDrop(
        (paths) => dropRef.current(paths),
        (state) => dragRef.current(state),
      );
      if (cancelled) unlisten?.();
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
