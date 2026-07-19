/**
 * ARC-07: shared list-query generation so stale `listTasksCursor` responses
 * from TaskList, AppShell.refreshTasks, or event-driven reloads cannot overwrite
 * a newer query.
 */

let listQueryEpoch = 0;

export function getListQueryEpoch(): number {
  return listQueryEpoch;
}

/** Bump before starting a replace-style reload. Returns the new epoch. */
export function bumpListQueryEpoch(): number {
  listQueryEpoch += 1;
  return listQueryEpoch;
}

export function isCurrentListQueryEpoch(captured: number): boolean {
  return captured === listQueryEpoch;
}

/** Test-only reset so cases do not leak epoch across files. */
export function resetListQueryEpochForTests(value = 0): void {
  listQueryEpoch = value;
}

export type ListLoadFlight = {
  replaceInFlight: boolean;
  appendInFlight: boolean;
  pendingReload: boolean;
};

export function createListLoadFlight(): ListLoadFlight {
  return {
    replaceInFlight: false,
    appendInFlight: false,
    pendingReload: false,
  };
}

export type BeginListLoadResult = { kind: "skip" } | { kind: "start"; epoch: number; role: "replace" | "append" };

/**
 * Decide whether a load may start. Replace bumps the global epoch so any
 * in-flight append/replace with an older epoch is ignored on apply.
 */
export function beginListLoad(flight: ListLoadFlight, append: boolean): BeginListLoadResult {
  if (append) {
    if (flight.replaceInFlight || flight.appendInFlight) {
      return { kind: "skip" };
    }
    flight.appendInFlight = true;
    return { kind: "start", epoch: getListQueryEpoch(), role: "append" };
  }

  const epoch = bumpListQueryEpoch();
  if (flight.replaceInFlight) {
    // Another replace is already running; request a follow-up with the latest query.
    flight.pendingReload = true;
    return { kind: "skip" };
  }
  flight.replaceInFlight = true;
  flight.pendingReload = false;
  return { kind: "start", epoch, role: "replace" };
}

export function endListLoad(flight: ListLoadFlight, role: "replace" | "append"): boolean {
  if (role === "replace") {
    flight.replaceInFlight = false;
  } else {
    flight.appendInFlight = false;
  }
  if (!flight.pendingReload) return false;
  if (flight.replaceInFlight || flight.appendInFlight) return false;
  flight.pendingReload = false;
  return true;
}
