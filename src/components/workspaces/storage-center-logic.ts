/**
 * Pure logic for the Storage & Cleanup Center: artifact grouping, byte
 * totals, cleanup-mode targeting, and result summarization. Kept free of
 * React/Tauri so it can be unit-tested directly (workspace-logic pattern).
 */
import type {
  ArtifactKind,
  ArtifactReason,
  CleanupMode,
  StorageArtifactItem,
  StorageCleanupResult,
  StorageScanResult,
} from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";

/** Reason code → i18n key. Walk-tested across all locales (see test file). */
export const STORAGE_REASON_KEYS: Record<ArtifactReason, TranslationKey> = {
  no_owner: "storageCenter.reason.no_owner",
  owner_completed: "storageCenter.reason.owner_completed",
  dht_stale: "storageCenter.reason.dht_stale",
  owner_resumable: "storageCenter.reason.owner_resumable",
};

/** Artifact kind → i18n section label. */
export const STORAGE_KIND_KEYS: Record<ArtifactKind, TranslationKey> = {
  temp_file: "storageCenter.kind.temp_file",
  legacy_temp_file: "storageCenter.kind.legacy_temp_file",
  staging_dir: "storageCenter.kind.staging_dir",
  publish_staging: "storageCenter.kind.publish_staging",
  metalink_part: "storageCenter.kind.metalink_part",
  dht_state: "storageCenter.kind.dht_state",
};

export const STORAGE_SWEEP_MODE_KEYS: Record<string, TranslationKey> = {
  startup: "storageCenter.sweepMode.startup",
  manual_orphans: "storageCenter.sweepMode.manual_orphans",
  manual_completed: "storageCenter.sweepMode.manual_completed",
  manual_all: "storageCenter.sweepMode.manual_all",
  manual_selected: "storageCenter.sweepMode.manual_selected",
  task_abandon: "storageCenter.sweepMode.task_abandon",
};

export function parseBytes(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

export interface ReclaimableGroup {
  kind: ArtifactKind;
  items: StorageArtifactItem[];
  totalBytes: number;
}

/** Group reclaimable items by kind, largest group first. */
export function groupReclaimable(items: StorageArtifactItem[]): ReclaimableGroup[] {
  const groups = new Map<ArtifactKind, ReclaimableGroup>();
  for (const item of items) {
    if (!item.reclaimable) {
      continue;
    }
    const group = groups.get(item.kind) ?? { kind: item.kind, items: [], totalBytes: 0 };
    group.items.push(item);
    group.totalBytes += parseBytes(item.bytes);
    groups.set(item.kind, group);
  }
  return [...groups.values()]
    .map((group) => ({
      ...group,
      items: [...group.items].sort((a, b) => parseBytes(b.bytes) - parseBytes(a.bytes)),
    }))
    .sort((a, b) => b.totalBytes - a.totalBytes);
}

export interface ResumableTaskGroup {
  taskId: string;
  taskFileName: string;
  protocol: string;
  items: StorageArtifactItem[];
  totalBytes: number;
}

/** Group kept (resumable) items by their owning task for the read-only view. */
export function groupResumableByTask(items: StorageArtifactItem[]): ResumableTaskGroup[] {
  const groups = new Map<string, ResumableTaskGroup>();
  for (const item of items) {
    if (item.reclaimable || !item.ownerTaskId) {
      continue;
    }
    const group =
      groups.get(item.ownerTaskId) ??
      ({
        taskId: item.ownerTaskId,
        taskFileName: item.taskFileName ?? item.ownerTaskId,
        protocol: item.ownerProtocol ?? "",
        items: [],
        totalBytes: 0,
      } satisfies ResumableTaskGroup);
    group.items.push(item);
    group.totalBytes += parseBytes(item.bytes);
    groups.set(item.ownerTaskId, group);
  }
  return [...groups.values()].sort((a, b) => b.totalBytes - a.totalBytes);
}

export interface StorageTotals {
  reclaimableBytes: number;
  reclaimableCount: number;
  resumableBytes: number;
}

export function totalsFor(items: StorageArtifactItem[]): StorageTotals {
  let reclaimableBytes = 0;
  let reclaimableCount = 0;
  let resumableBytes = 0;
  for (const item of items) {
    if (item.reclaimable) {
      reclaimableBytes += parseBytes(item.bytes);
      reclaimableCount += 1;
    } else {
      resumableBytes += parseBytes(item.bytes);
    }
  }
  return { reclaimableBytes, reclaimableCount, resumableBytes };
}

/** Which items an aggregate cleanup mode would target right now. */
export function itemsForMode(items: StorageArtifactItem[], mode: CleanupMode): StorageArtifactItem[] {
  return items.filter((item) => {
    if (mode === "orphans") {
      return item.reclaimable && item.reason === "no_owner";
    }
    if (mode === "completed_leftovers") {
      return item.reclaimable && item.reason === "owner_completed";
    }
    return item.reclaimable;
  });
}

export interface CleanupReport {
  removedCount: number;
  skippedCount: number;
  failedCount: number;
  failures: { itemId: string; errorCode: string | null }[];
}

/** Summarize a cleanup result; failures are listed individually so partial
 * success is never displayed as full success. */
export function summarizeCleanup(result: StorageCleanupResult): CleanupReport {
  const failures = result.outcomes
    .filter((outcome) => outcome.outcome === "failed")
    .map((outcome) => ({ itemId: outcome.itemId, errorCode: outcome.errorCode }));
  return {
    removedCount: result.removedCount,
    skippedCount: result.skippedCount,
    failedCount: result.failedCount,
    failures,
  };
}

/** Whether a scan is worth rendering as "empty" (nothing reclaimable). */
export function scanHasReclaimable(scan: StorageScanResult | null): boolean {
  return scan?.items.some((item) => item.reclaimable) ?? false;
}
