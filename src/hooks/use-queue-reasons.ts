import { useEffect, useMemo, useState } from "react";

import type { QueueTaskDecision } from "@/generated/bindings";
import { getSchedulerSnapshot } from "@/lib/tauri";
import { useTaskDataStore } from "@/stores/task-store";

/** Matches QueueCenter's poll cadence so the row and the queue view never
 * disagree about why a task is waiting. */
const REFRESH_INTERVAL_MS = 10_000;

interface QueueReasonSnapshot {
  key: string;
  decisions: QueueTaskDecision[];
}

/** Scheduler wait reasons for the queued tasks among `taskIds`.
 *
 * A queued row has to explain *why* it is waiting, and that answer only exists
 * in the scheduler — the task record just says "queued". One shared poll per
 * list keeps the lookup off the per-row render path.
 *
 * The returned Map is referentially stable while the reasons are unchanged, so
 * memoized rows do not re-render on every poll.
 */
export function useQueueReasons(taskIds: string[]): Map<string, QueueTaskDecision> {
  // A joined key (not an array) so the selector compares by value and progress
  // ticks never invalidate it.
  const queuedIdsKey = useTaskDataStore((state) => {
    const queued: string[] = [];
    for (const id of taskIds) {
      if (state.taskById[id]?.status === "queued") queued.push(id);
    }
    return queued.join("\u0000");
  });

  const [snapshot, setSnapshot] = useState<QueueReasonSnapshot | null>(null);

  useEffect(() => {
    if (!queuedIdsKey) {
      setSnapshot((previous) => (previous === null ? previous : null));
      return;
    }
    let cancelled = false;
    const load = async () => {
      try {
        const next = await getSchedulerSnapshot(queuedIdsKey.split("\u0000"));
        if (cancelled) return;
        // Reason text is an enhancement, not row-critical state, so a changed
        // *set* of ids matters but an unchanged answer must not churn the Map.
        const key = next.decisions
          .map((decision) => `${decision.taskId}:${decision.reason}:${decision.hostUsedSlots}`)
          .join("\u0000");
        setSnapshot((previous) => (previous?.key === key ? previous : { key, decisions: next.decisions }));
      } catch {
        // Keep the previous snapshot; the row falls back to generic queued copy.
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [queuedIdsKey]);

  return useMemo(() => new Map(snapshot?.decisions.map((decision) => [decision.taskId, decision]) ?? []), [snapshot]);
}
